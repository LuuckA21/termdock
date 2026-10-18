//! Uploads and downloads, of files and whole folders, in two steps: a plan
//! (what will be copied and what already exists), then the run with the
//! user's choice for what exists (replace all or skip all).
//!
//! The rules of single files hold for folders too: local paths only come
//! from native dialogs or drops (never from the UI); links are not followed
//! inside folders on either side; remote names are untrusted and only become
//! local names after `local_name_in_folder`; local folders that turn out to be
//! links are never written through.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use termdock_core::Error as CoreError;
use termdock_core::sftp::{
    CancellationToken, EntryKind, FileType, MAX_TREE_DEPTH, MAX_TREE_ITEMS, Sftp, Tree, TreeItem, join, validate_name,
};
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::commands::{CmdError, CmdResult, blocking};
use crate::sftp::{SftpState, Source, dialog, local_file_name, local_name_in_folder, open_regular};

/// A plan the user did not start is dropped (with the files it holds open).
const PLAN_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_PLANS: usize = 16;
/// After this many failures in a row the connection is most likely gone.
const MAX_FAILURES_IN_A_ROW: usize = 5;
/// Names of existing items shown when asking, and errors reported.
const MAX_EXAMPLES: usize = 5;
/// Progress messages are sent at most this often (and at each new file).
const PROGRESS_STEP: u64 = 256 * 1024;

enum Local {
    /// A file picked on its own, opened when it was picked.
    Open(std::fs::File, Option<u32>),
    /// A file inside a picked folder, opened when its turn comes.
    Path(PathBuf),
}

enum Step {
    /// A remote folder, created unless it already exists as a folder.
    RemoteDir { dir: String, name: String },
    Upload {
        source: Local,
        dir: String,
        name: String,
        exists: bool,
    },
    /// A local folder, created unless it already exists as a real folder.
    LocalDir { path: PathBuf },
    Download {
        dir: String,
        name: String,
        local: PathBuf,
        exists: bool,
        /// The user chose this exact path (save dialog): written even if a
        /// file is there, the dialog already asked.
        chosen: bool,
    },
}

struct Planned {
    step: Step,
    /// What the user sees: the path below the transferred folder.
    label: String,
    size: u64,
    /// The folder step this one goes into, if part of a folder.
    parent: Option<usize>,
}

/// A transfer waiting for the user's go.
pub(crate) struct Plan {
    pub(crate) browser: u32,
    steps: Vec<Planned>,
    at: Instant,
}

/// What a plan will do, for the UI to show and ask about.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    plan: u32,
    files: usize,
    folders: usize,
    bytes: u64,
    /// Files that already exist at the destination.
    existing: usize,
    /// A few of them, by path below the destination.
    examples: Vec<String>,
    /// Left out: links, special files, unusable names, name clashes.
    skipped: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunProgress {
    transfer: u32,
    /// Bytes copied so far, of `total`.
    done: u64,
    total: u64,
    /// Files finished, of `count`.
    files: usize,
    count: usize,
    /// The file being copied.
    current: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    copied: usize,
    skipped: usize,
    failed: usize,
    /// The first errors, as "path: reason".
    errors: Vec<String>,
    /// Stopped early after repeated failures.
    stopped: bool,
}

#[derive(Default)]
struct Builder {
    steps: Vec<Planned>,
    skipped: usize,
}

impl Builder {
    fn push(&mut self, step: Step, label: String, size: u64, parent: Option<usize>) -> usize {
        self.steps.push(Planned {
            step,
            label,
            size,
            parent,
        });
        self.steps.len() - 1
    }

    fn info(&self, plan: u32) -> PlanInfo {
        let mut info = PlanInfo {
            plan,
            files: 0,
            folders: 0,
            bytes: 0,
            existing: 0,
            examples: Vec::new(),
            skipped: self.skipped,
        };
        for p in &self.steps {
            match &p.step {
                Step::RemoteDir { .. } | Step::LocalDir { .. } => info.folders += 1,
                Step::Upload { exists, .. } | Step::Download { exists, .. } => {
                    info.files += 1;
                    info.bytes += p.size;
                    if *exists {
                        info.existing += 1;
                        if info.examples.len() < MAX_EXAMPLES {
                            info.examples.push(p.label.clone());
                        }
                    }
                }
            }
        }
        info
    }

    fn store(self, sftp: &SftpState, browser: u32) -> CmdResult<PlanInfo> {
        let id = sftp.next_id();
        let info = self.info(id);
        let mut plans = sftp.plans().lock().unwrap();
        plans.retain(|_, p| p.at.elapsed() < PLAN_TTL);
        if plans.len() >= MAX_PLANS {
            return Err(CmdError::msg("too many transfers waiting: start or cancel some first"));
        }
        plans.insert(
            id,
            Plan {
                browser,
                steps: self.steps,
                at: Instant::now(),
            },
        );
        Ok(info)
    }
}

fn label(path: &[String]) -> String {
    path.join("/")
}

/// The names in a remote folder and what they are (links not followed).
type RemoteNames = HashMap<String, EntryKind>;

async fn remote_names(fs: &Sftp, path: &str) -> CmdResult<RemoteNames> {
    Ok(fs
        .list(path)
        .await?
        .entries
        .into_iter()
        .map(|e| (e.name, e.kind))
        .collect())
}

/// What a picked local folder holds, each folder before its content. Links
/// and special files are left out, like names that could not be used on the
/// server; the same limits as remote folders apply.
fn local_tree(root: &Path) -> CmdResult<(Tree, Vec<PathBuf>)> {
    let mut tree = Tree::default();
    let mut paths = Vec::new();
    let mut pending = vec![(root.to_path_buf(), Vec::<String>::new())];
    while let Some((dir, below)) = pending.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| CmdError::msg(format!("{}: {e}", dir.display())))?;
        for entry in entries {
            let Ok(entry) = entry else {
                tree.skipped += 1;
                continue;
            };
            // The entry's own type: a link is a link, not what it points to.
            let (Ok(kind), Ok(name)) = (entry.file_type(), entry.file_name().into_string()) else {
                tree.skipped += 1;
                continue;
            };
            let mut path = below.clone();
            path.push(name);
            let usable = path.last().is_some_and(|n| validate_name(n).is_ok());
            if usable && kind.is_dir() && path.len() < MAX_TREE_DEPTH {
                pending.push((entry.path(), path.clone()));
                tree.items.push(TreeItem {
                    path,
                    dir: true,
                    size: 0,
                });
                paths.push(entry.path());
            } else if usable && kind.is_file() {
                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                tree.items.push(TreeItem { path, dir: false, size });
                paths.push(entry.path());
            } else {
                tree.skipped += 1;
            }
            if tree.items.len() > MAX_TREE_ITEMS {
                return Err(CoreError::TooManyItems(MAX_TREE_ITEMS).into());
            }
        }
    }
    Ok((tree, paths))
}

/// Plans uploading the picked files and folders (tokens from the open
/// dialogs or a drop) into the remote folder `dir`. The tokens are used up.
#[tauri::command]
pub async fn sftp_plan_upload(
    sftp: State<'_, SftpState>,
    id: u32,
    dir: String,
    tokens: Vec<Uuid>,
) -> CmdResult<PlanInfo> {
    let fs = sftp.browser(id)?;
    let picked: Vec<_> = tokens.iter().filter_map(|t| sftp.take_picked(*t)).collect();
    if picked.is_empty() {
        return Err(CmdError::msg("pick the files again"));
    }
    let here = remote_names(&fs, &dir).await?;
    let mut plan = Builder::default();
    for item in picked {
        let name = item.name;
        if validate_name(&name).is_err() {
            plan.skipped += 1;
            continue;
        }
        match item.source {
            Source::File(file, mode) => match here.get(&name) {
                None | Some(EntryKind::File | EntryKind::Link) => {
                    let size = file.metadata().map(|m| m.len()).unwrap_or(0);
                    let exists = here.contains_key(&name);
                    plan.push(
                        Step::Upload {
                            source: Local::Open(file, mode),
                            dir: dir.clone(),
                            name: name.clone(),
                            exists,
                        },
                        name,
                        size,
                        None,
                    );
                }
                // A folder or something special by that name: never replaced.
                Some(_) => plan.skipped += 1,
            },
            Source::Folder(path) => {
                let (tree, paths) = blocking(move || local_tree(&path)).await??;
                plan_upload_folder(&fs, &mut plan, &dir, &name, here.get(&name).copied(), tree, paths).await?;
            }
        }
    }
    plan.store(&sftp, id)
}

/// Adds a local folder (already read into `tree`) uploaded as `dir/name`.
/// Remote folders that exist are merged into; the names inside them are
/// listed to find the files that exist. A remote link or file where a folder
/// goes keeps that whole branch out.
async fn plan_upload_folder(
    fs: &Sftp,
    plan: &mut Builder,
    dir: &str,
    name: &str,
    existing: Option<EntryKind>,
    tree: Tree,
    paths: Vec<PathBuf>,
) -> CmdResult<()> {
    let root = match existing {
        None | Some(EntryKind::Dir) => plan.push(
            Step::RemoteDir {
                dir: dir.to_owned(),
                name: name.to_owned(),
            },
            name.to_owned(),
            0,
            None,
        ),
        Some(_) => {
            plan.skipped += 1 + tree.items.len() + tree.skipped;
            return Ok(());
        }
    };
    plan.skipped += tree.skipped;
    let root_path = join(dir, name);
    // For each planned folder: its remote path, its step, and the names it
    // already holds (`None`: a new folder, nothing in it yet).
    let root_names = match existing {
        Some(_) => Some(remote_names(fs, &root_path).await?),
        None => None,
    };
    let mut folders: HashMap<Vec<String>, (String, usize, Option<RemoteNames>)> = HashMap::new();
    folders.insert(Vec::new(), (root_path, root, root_names));
    for (item, local) in tree.items.into_iter().zip(paths) {
        let (parent_path, leaf) = item.path.split_at(item.path.len() - 1);
        let leaf = leaf[0].clone();
        // A folder that was left out takes its content with it.
        let Some((remote_dir, parent, names)) = folders.get(parent_path) else {
            plan.skipped += 1;
            continue;
        };
        let (remote_dir, parent) = (remote_dir.clone(), *parent);
        let found = names.as_ref().and_then(|n| n.get(&leaf)).copied();
        let shown = format!("{name}/{}", label(&item.path));
        if item.dir {
            match found {
                None | Some(EntryKind::Dir) => {
                    let step = plan.push(
                        Step::RemoteDir {
                            dir: remote_dir.clone(),
                            name: leaf.clone(),
                        },
                        shown,
                        0,
                        Some(parent),
                    );
                    let path = join(&remote_dir, &leaf);
                    let inside = match found {
                        Some(_) => Some(remote_names(fs, &path).await?),
                        None => None,
                    };
                    folders.insert(item.path, (path, step, inside));
                }
                Some(_) => plan.skipped += 1,
            }
        } else {
            match found {
                None | Some(EntryKind::File | EntryKind::Link) => {
                    plan.push(
                        Step::Upload {
                            source: Local::Path(local),
                            dir: remote_dir,
                            name: leaf,
                            exists: found.is_some(),
                        },
                        shown,
                        item.size,
                        Some(parent),
                    );
                }
                Some(_) => plan.skipped += 1,
            }
        }
    }
    Ok(())
}

/// What is at `path` locally, without following a link.
enum LocalKind {
    Missing,
    Dir,
    /// A file or a link: replaced as a whole (a link is never written through).
    Replaceable,
    Other,
}

fn local_kind(path: &Path) -> LocalKind {
    match std::fs::symlink_metadata(path) {
        Err(_) => LocalKind::Missing,
        Ok(m) if m.is_dir() => LocalKind::Dir,
        Ok(m) if m.is_file() || m.file_type().is_symlink() => LocalKind::Replaceable,
        Ok(_) => LocalKind::Other,
    }
}

/// A name for the local copy that no other item in the same folder has
/// taken (case-insensitive, as on Windows and macOS).
fn claim(taken: &mut HashSet<(PathBuf, String)>, dir: &Path, remote: &str) -> Option<String> {
    let local = local_name_in_folder(remote);
    taken.insert((dir.to_path_buf(), local.to_lowercase())).then_some(local)
}

/// Asks where to save the remote file or folder `dir/name` (a save dialog
/// for a file, a folder dialog for a folder), then plans the download.
/// `None` when the user cancelled the dialog.
#[tauri::command]
pub async fn sftp_plan_download(
    app: AppHandle,
    sftp: State<'_, SftpState>,
    id: u32,
    dir: String,
    name: String,
) -> CmdResult<Option<PlanInfo>> {
    let fs = sftp.browser(id)?;
    // A link counts as what it points to.
    let (_, _, kind) = fs.file_info(&dir, &name).await?;
    let downloads = directories::UserDirs::new().and_then(|d| d.download_dir().map(Path::to_path_buf));
    let (tx, rx) = oneshot::channel();
    let mut picker = dialog(&app);
    if let Some(d) = downloads {
        picker = picker.set_directory(d);
    }
    let folder = kind == FileType::Dir;
    if folder {
        picker.pick_folder(move |picked| {
            let _ = tx.send(picked);
        });
    } else {
        picker.set_file_name(local_file_name(&name)).save_file(move |picked| {
            let _ = tx.send(picked);
        });
    }
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let picked = picked.into_path().map_err(|e| CmdError::msg(e.to_string()))?;
    if !picked.is_absolute() {
        return Err(CmdError::msg("choose a local folder"));
    }
    // The dialog may have stayed open while the connection closed.
    let fs = sftp.browser(id)?;
    let mut plan = Builder::default();
    if !folder {
        // The save dialog already asked before replacing an existing file.
        let size = fs.file_info(&dir, &name).await?.0;
        plan.push(
            Step::Download {
                dir,
                name: name.clone(),
                local: picked,
                exists: false,
                chosen: true,
            },
            name,
            size,
            None,
        );
        return plan.store(&sftp, id).map(Some);
    }
    let (root, tree) = fs.walk(&dir, &name).await?;
    let plan = blocking(move || plan_download_folder(plan, &picked, &root, &name, tree)).await??;
    plan.store(&sftp, id).map(Some)
}

/// Adds the remote folder `root` (read into `tree`), downloaded into the
/// local folder `dest` under a safe version of `name`. Existing local
/// folders are merged into; a local file or link where a folder goes keeps
/// that branch out.
fn plan_download_folder(mut plan: Builder, dest: &Path, root: &str, name: &str, tree: Tree) -> CmdResult<Builder> {
    let mut taken = HashSet::new();
    let shown_root = local_name_in_folder(name);
    let root_local = dest.join(&shown_root);
    let root_kind = local_kind(&root_local);
    if !matches!(root_kind, LocalKind::Missing | LocalKind::Dir) {
        plan.skipped += 1 + tree.items.len() + tree.skipped;
        return Ok(plan);
    }
    plan.skipped += tree.skipped;
    let step = plan.push(
        Step::LocalDir {
            path: root_local.clone(),
        },
        shown_root.clone(),
        0,
        None,
    );
    // For each planned folder: remote path, local path, step, and whether it
    // already existed locally (so its content must be checked).
    let mut folders: HashMap<Vec<String>, (String, PathBuf, usize, bool)> = HashMap::new();
    folders.insert(
        Vec::new(),
        (root.to_owned(), root_local, step, matches!(root_kind, LocalKind::Dir)),
    );
    for item in tree.items {
        let (parent_path, leaf) = item.path.split_at(item.path.len() - 1);
        let leaf = leaf[0].clone();
        let Some((remote_dir, local_dir, parent, existed)) = folders.get(parent_path).cloned() else {
            plan.skipped += 1;
            continue;
        };
        // Two remote names that become the same local name: the first wins.
        let Some(local_name) = claim(&mut taken, &local_dir, &leaf) else {
            plan.skipped += 1;
            continue;
        };
        let local = local_dir.join(&local_name);
        let kind = if existed {
            local_kind(&local)
        } else {
            LocalKind::Missing
        };
        let shown = format!("{shown_root}/{}", {
            let mut path: Vec<String> = item.path[..item.path.len() - 1].to_vec();
            path.push(local_name);
            label(&path)
        });
        if item.dir {
            if !matches!(kind, LocalKind::Missing | LocalKind::Dir) {
                plan.skipped += 1;
                continue;
            }
            let step = plan.push(Step::LocalDir { path: local.clone() }, shown, 0, Some(parent));
            folders.insert(
                item.path,
                (join(&remote_dir, &leaf), local, step, matches!(kind, LocalKind::Dir)),
            );
        } else {
            let exists = match kind {
                LocalKind::Missing => false,
                LocalKind::Replaceable => true,
                LocalKind::Dir | LocalKind::Other => {
                    plan.skipped += 1;
                    continue;
                }
            };
            plan.push(
                Step::Download {
                    dir: remote_dir,
                    name: leaf,
                    local,
                    exists,
                    chosen: false,
                },
                shown,
                item.size,
                Some(parent),
            );
        }
    }
    Ok(plan)
}

/// A plan the user decided not to start.
#[tauri::command]
pub fn sftp_drop_plan(sftp: State<'_, SftpState>, plan: u32) {
    sftp.plans().lock().unwrap().remove(&plan);
}

/// Runs a plan. Existing files are replaced if `replace`, otherwise left as
/// they are. A failed item does not stop the others, but repeated failures
/// in a row do (the connection is likely gone). Cancel with `sftp_cancel`.
#[tauri::command]
pub async fn sftp_run_plan(
    sftp: State<'_, SftpState>,
    plan: u32,
    replace: bool,
    on_progress: Channel<RunProgress>,
) -> CmdResult<RunSummary> {
    let plan = sftp
        .plans()
        .lock()
        .unwrap()
        .remove(&plan)
        .filter(|p| p.at.elapsed() < PLAN_TTL)
        .ok_or_else(|| CmdError::msg("this transfer expired: start it again"))?;
    let fs = sftp.browser(plan.browser)?;
    let (transfer, cancel) = sftp.track(plan.browser)?;
    let result = run(&fs, plan.steps, replace, transfer, &cancel, &on_progress).await;
    sftp.end_transfer(transfer);
    result
}

async fn run(
    fs: &Arc<Sftp>,
    steps: Vec<Planned>,
    replace: bool,
    transfer: u32,
    cancel: &CancellationToken,
    channel: &Channel<RunProgress>,
) -> CmdResult<RunSummary> {
    let count = steps
        .iter()
        .filter(|p| matches!(p.step, Step::Upload { .. } | Step::Download { .. }))
        .count();
    let total: u64 = steps.iter().map(|p| p.size).sum();
    let mut summary = RunSummary {
        copied: 0,
        skipped: 0,
        failed: 0,
        errors: Vec::new(),
        stopped: false,
    };
    // Folder steps that could not be created: what goes into them is skipped.
    let mut broken = vec![false; steps.len()];
    let mut in_a_row = 0;
    let mut base = 0u64;
    let mut files = 0usize;
    let send = |done: u64, files: usize, current: &str| {
        let _ = channel.send(RunProgress {
            transfer,
            done,
            total,
            files,
            count,
            current: current.to_owned(),
        });
    };
    // The UI learns the id at once, so Cancel works from the start.
    send(0, 0, "");
    for (i, planned) in steps.into_iter().enumerate() {
        if cancel.is_cancelled() {
            return Err(CoreError::Cancelled.into());
        }
        let Planned {
            step,
            label,
            size,
            parent,
        } = planned;
        let is_file = matches!(step, Step::Upload { .. } | Step::Download { .. });
        if parent.is_some_and(|p| broken[p]) {
            broken[i] = true;
            if is_file {
                summary.skipped += 1;
                files += 1;
                base += size;
            }
            continue;
        }
        if is_file {
            send(base, files, &label);
        }
        let mut reported = 0u64;
        let progress = |done: u64, _: Option<u64>| {
            if done - reported >= PROGRESS_STEP {
                reported = done;
                send(base + done, files, &label);
            }
        };
        let outcome = match step {
            Step::RemoteDir { dir, name } => remote_dir(fs, &dir, &name).await.map(|_| false),
            Step::LocalDir { path } => local_dir(&path).await.map(|_| false),
            Step::Upload { exists, .. } | Step::Download { exists, .. } if exists && !replace => Ok(true),
            Step::Upload {
                source,
                dir,
                name,
                exists: _,
            } => upload(fs, source, &dir, &name, replace, cancel, progress).await,
            Step::Download {
                dir,
                name,
                local,
                exists: _,
                chosen,
            } => download(fs, &dir, &name, &local, replace || chosen, cancel, progress).await,
        };
        if is_file {
            files += 1;
            base += size;
        }
        match outcome {
            Ok(skipped) => {
                in_a_row = 0;
                if is_file && skipped {
                    summary.skipped += 1;
                } else if is_file {
                    summary.copied += 1;
                }
            }
            Err(CoreError::Cancelled) => return Err(CoreError::Cancelled.into()),
            Err(e) => {
                broken[i] = true;
                summary.failed += 1;
                in_a_row += 1;
                if summary.errors.len() < MAX_EXAMPLES {
                    summary.errors.push(format!("{label}: {e}"));
                }
                if in_a_row >= MAX_FAILURES_IN_A_ROW {
                    summary.stopped = true;
                    break;
                }
            }
        }
    }
    send(base, files, "");
    Ok(summary)
}

async fn remote_dir(fs: &Sftp, dir: &str, name: &str) -> Result<(), CoreError> {
    match fs.mkdir(dir, name).await {
        Ok(()) => Ok(()),
        // Already there (or created meanwhile): fine if it is a real folder.
        Err(e) => match fs.lookup(dir, name).await {
            Ok(Some(EntryKind::Dir)) => Ok(()),
            _ => Err(e),
        },
    }
}

async fn local_dir(path: &Path) -> Result<(), CoreError> {
    match tokio::fs::create_dir(path).await {
        Ok(()) => Ok(()),
        // Only a real folder is used; a link is never followed.
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => match tokio::fs::symlink_metadata(path).await {
            Ok(m) if m.is_dir() => Ok(()),
            _ => Err(CoreError::Exists(path.display().to_string())),
        },
        Err(e) => Err(e.into()),
    }
}

/// Uploads one file. `Ok(true)`: skipped, it appeared meanwhile.
async fn upload(
    fs: &Sftp,
    source: Local,
    dir: &str,
    name: &str,
    replace: bool,
    cancel: &CancellationToken,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<bool, CoreError> {
    let (file, mode) = match source {
        Local::Open(file, mode) => (file, mode),
        Local::Path(path) => {
            let opened = tokio::task::spawn_blocking(move || {
                // Still a regular file, not a link put there meanwhile.
                std::fs::symlink_metadata(&path)
                    .ok()
                    .filter(|m| m.is_file())
                    .and_then(|_| open_regular(&path))
            })
            .await
            .map_err(|_| CoreError::Cancelled)?;
            let (file, _, mode) = opened.ok_or_else(|| CoreError::NotRegular(name.chars().take(64).collect()))?;
            (file, mode)
        }
    };
    match fs.upload(file, mode, dir, name, replace, cancel, progress).await {
        Ok(_) => Ok(false),
        Err(CoreError::Exists(_)) if !replace => Ok(true),
        Err(e) => Err(e),
    }
}

/// Downloads one file. `Ok(true)`: skipped, it appeared meanwhile.
async fn download(
    fs: &Sftp,
    dir: &str,
    name: &str,
    local: &Path,
    replace: bool,
    cancel: &CancellationToken,
    progress: impl FnMut(u64, Option<u64>),
) -> Result<bool, CoreError> {
    if !replace && tokio::fs::symlink_metadata(local).await.is_ok() {
        return Ok(true);
    }
    fs.download(dir, name, local, cancel, progress).await.map(|_| false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    #[test]
    fn local_tree_skips_links_and_keeps_order() {
        let dir = tempdir();
        let root = dir.path().join("pkg");
        std::fs::create_dir_all(root.join("a/b")).unwrap();
        std::fs::write(root.join("a/b/f.txt"), b"12345").unwrap();
        std::fs::write(root.join("top.txt"), b"x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc", root.join("escape")).unwrap();
        let (tree, paths) = local_tree(&root).unwrap();
        assert_eq!(tree.items.len(), paths.len());
        let names: Vec<String> = tree.items.iter().map(|i| label(&i.path)).collect();
        assert!(names.contains(&"a/b/f.txt".to_owned()));
        assert!(!names.iter().any(|n| n.starts_with("escape")));
        // Every folder comes before what it holds.
        let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
        assert!(pos("a") < pos("a/b") && pos("a/b") < pos("a/b/f.txt"));
        let f = tree.items.iter().find(|i| label(&i.path) == "a/b/f.txt").unwrap();
        assert_eq!(f.size, 5);
        #[cfg(unix)]
        assert_eq!(tree.skipped, 1);
    }

    #[test]
    fn download_plan_sanitises_and_never_writes_through_links() {
        let dest = tempdir();
        let item = |path: &[&str], dir: bool| TreeItem {
            path: path.iter().map(|s| s.to_string()).collect(),
            dir,
            size: 3,
        };
        // An existing local copy whose "sub" is a link elsewhere.
        std::fs::create_dir(dest.path().join("site")).unwrap();
        std::fs::write(dest.path().join("site").join("index.html"), b"old").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dest.path(), dest.path().join("site").join("sub")).unwrap();
        let tree = Tree {
            items: vec![
                item(&["index.html"], false),
                item(&["sub"], true),
                item(&["sub", "x"], false),
                item(&["a:b"], false),
                item(&["a_b"], false),
                item(&["..hidden"], false),
            ],
            skipped: 0,
        };
        let plan = plan_download_folder(Builder::default(), dest.path(), "/srv/site", "site", tree).unwrap();
        let info = plan.info(1);
        // index.html exists: a conflict; "a:b" and "a_b" clash locally.
        assert_eq!(info.existing, 1);
        assert_eq!(info.examples, vec!["site/index.html".to_owned()]);
        let locals: Vec<PathBuf> = plan
            .steps
            .iter()
            .filter_map(|p| match &p.step {
                Step::Download { local, .. } | Step::LocalDir { path: local } => Some(local.clone()),
                _ => None,
            })
            .collect();
        for l in &locals {
            assert!(l.starts_with(dest.path().join("site")), "{}", l.display());
        }
        assert!(locals.contains(&dest.path().join("site").join("a_b")));
        assert!(locals.contains(&dest.path().join("site").join(".hidden")));
        #[cfg(unix)]
        {
            // The link "sub" and its content are left out, as is the clash.
            assert!(
                !locals
                    .iter()
                    .any(|l| l.starts_with(dest.path().join("site").join("sub")))
            );
            assert_eq!(plan.skipped, 3);
        }
    }
}
