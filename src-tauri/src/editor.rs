//! The program *Edit* opens files with: the system's text editor, or one the
//! user picked in Settings. A program is only ever chosen in a native file
//! dialog opened here (the UI can ask for the dialog, never name a program),
//! kept in `editor.json` in the config folder and checked again each time it
//! is used. It is started directly, with the file as its only argument: no
//! shell, no file associations.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tokio::sync::oneshot;

use crate::commands::{CmdError, CmdResult, blocking};

const MAX_FILE_SIZE: u64 = 64 * 1024;

#[derive(Default, Serialize, Deserialize)]
struct Document {
    version: u32,
    /// Absolute path of the chosen program (on macOS, an `.app` bundle).
    program: Option<PathBuf>,
}

pub struct EditorChoice {
    file: PathBuf,
    program: Mutex<Option<PathBuf>>,
}

/// What Settings shows: the program's name, and its full path as a hint.
#[derive(Serialize)]
pub struct EditorInfo {
    name: String,
    path: String,
}

impl EditorChoice {
    /// Reads the saved choice; an unreadable file means the system editor.
    pub fn load(config_dir: &Path) -> Self {
        let file = config_dir.join("editor.json");
        let program = std::fs::metadata(&file)
            .ok()
            .filter(|m| m.len() <= MAX_FILE_SIZE)
            .and_then(|_| std::fs::read(&file).ok())
            .and_then(|bytes| serde_json::from_slice::<Document>(&bytes).ok())
            .and_then(|doc| doc.program)
            .filter(|p| p.is_absolute());
        Self {
            file,
            program: Mutex::new(program),
        }
    }

    /// Opens `path` in the chosen editor, or in the system's text editor.
    pub fn open(&self, path: &Path) -> Result<(), String> {
        let chosen = self.program.lock().unwrap().clone();
        let mut cmd = match chosen {
            Some(program) => {
                check_program(&program).map_err(|_| {
                    format!(
                        "the editor chosen in Settings ({}) is no longer there: choose it again or use the system editor",
                        program.display()
                    )
                })?;
                program_command(&program, path)
            }
            None => system_editor(path)?,
        };
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("could not open the text editor: {e}"))?;
        // Reap it when it exits (open/gio return at once; editors later).
        std::thread::spawn(move || {
            let _ = child.wait();
        });
        Ok(())
    }

    fn info(&self) -> Option<EditorInfo> {
        self.program.lock().unwrap().as_deref().map(info)
    }

    fn set(&self, program: Option<PathBuf>) -> CmdResult<()> {
        let doc = Document {
            version: 1,
            program: program.clone(),
        };
        let dir = self
            .file
            .parent()
            .ok_or_else(|| CmdError::msg("no configuration folder"))?;
        termdock_core::store::create_private_dir(dir)?;
        let io = |e: std::io::Error| CmdError::from(termdock_core::Error::from(e));
        let mut tmp = tempfile::Builder::new()
            .prefix(".editor")
            .suffix(".tmp")
            .tempfile_in(dir)
            .map_err(io)?;
        serde_json::to_writer_pretty(tmp.as_file_mut(), &doc).map_err(|e| CmdError::msg(e.to_string()))?;
        tmp.as_file_mut().write_all(b"\n").map_err(io)?;
        tmp.as_file_mut().sync_all().map_err(io)?;
        tmp.persist(&self.file).map_err(|e| io(e.error))?;
        *self.program.lock().unwrap() = program;
        Ok(())
    }
}

fn info(program: &Path) -> EditorInfo {
    EditorInfo {
        name: program
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: program.display().to_string(),
    }
}

/// The editor chosen in Settings; null for the system's text editor.
#[tauri::command]
pub fn editor_get(editor: State<'_, EditorChoice>) -> Option<EditorInfo> {
    editor.info()
}

/// Lets the user pick the editor in a native dialog (programs only).
/// Resolves with the new choice, or null if the dialog was cancelled.
#[tauri::command]
pub async fn editor_pick(app: AppHandle) -> CmdResult<Option<EditorInfo>> {
    let (tx, rx) = oneshot::channel();
    let mut dialog = crate::sftp::dialog(&app);
    if let Some(dir) = programs_folder() {
        dialog = dialog.set_directory(dir);
    }
    #[cfg(windows)]
    {
        dialog = dialog.add_filter("Programs", &["exe"]);
    }
    #[cfg(target_os = "macos")]
    {
        dialog = dialog.add_filter("Applications", &["app"]);
    }
    dialog.pick_file(move |picked| {
        let _ = tx.send(picked);
    });
    let Some(picked) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    let program = picked
        .into_path()
        .map_err(|_| CmdError::msg("this is not a local program"))?;
    // Checking the program and saving the choice touch the disk.
    blocking(move || {
        check_program(&program).map_err(CmdError::msg)?;
        let editor = app.state::<EditorChoice>();
        editor.set(Some(program))?;
        Ok(editor.info())
    })
    .await?
}

/// Goes back to the system's text editor (off the main thread: it writes a file).
#[tauri::command(async)]
pub fn editor_reset(editor: State<'_, EditorChoice>) -> CmdResult<()> {
    editor.set(None)
}

/// Where the dialog starts: the usual place for programs.
fn programs_folder() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("ProgramFiles").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        Some(PathBuf::from("/Applications"))
    } else {
        Some(PathBuf::from("/usr/bin"))
    }
}

/// A program that can be started directly: an `.exe` on Windows (no scripts
/// or shortcuts, which would go through another interpreter), an `.app` or
/// an executable file on macOS, an executable file elsewhere.
fn check_program(program: &Path) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "{} is not a program that can be used as editor",
            program.display()
        ))
    };
    if !program.is_absolute() || runs_files(program) {
        return refuse();
    }
    #[cfg(windows)]
    {
        let exe = program.extension().is_some_and(|e| e.eq_ignore_ascii_case("exe"));
        // App aliases (WindowsApps) are reparse points that cannot be
        // followed: check the entry itself.
        let file = std::fs::symlink_metadata(program).is_ok_and(|m| !m.is_dir());
        if exe && file { Ok(()) } else { refuse() }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let Ok(meta) = std::fs::metadata(program) else {
            return refuse();
        };
        let app = cfg!(target_os = "macos")
            && meta.is_dir()
            && program.extension().is_some_and(|e| e.eq_ignore_ascii_case("app"));
        if app || (meta.is_file() && meta.permissions().mode() & 0o111 != 0) {
            Ok(())
        } else {
            refuse()
        }
    }
}

/// Shells, interpreters and launchers: given the file, they would run the
/// server's content instead of opening it. Checked on the name chosen and,
/// for a link, on the program it points to.
fn runs_files(program: &Path) -> bool {
    const PREFIXES: &[&str] = &[
        "python",
        "perl",
        "ruby",
        "php",
        "lua",
        "node",
        "pwsh",
        "powershell",
        "wish",
        "tclsh",
    ];
    const NAMES: &[&str] = &[
        "sh",
        "bash",
        "dash",
        "zsh",
        "fish",
        "ksh",
        "mksh",
        "csh",
        "tcsh",
        "busybox",
        "env",
        "xargs",
        "sudo",
        "doas",
        "pkexec",
        "su",
        "nohup",
        "timeout",
        "nice",
        "xterm",
        "osascript",
        "open",
        "xdg-open",
        "gio",
        "gtk-launch",
        "cmd",
        "wscript",
        "cscript",
        "mshta",
        "rundll32",
        "regsvr32",
        "msiexec",
        "wsl",
        "conhost",
        "explorer",
        "start",
        "terminal",
        "iterm",
        "iterm2",
        "script editor",
        "automator",
    ];
    let real = std::fs::canonicalize(program).ok();
    [Some(program), real.as_deref()].into_iter().flatten().any(|path| {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        NAMES.contains(&stem.as_str()) || PREFIXES.iter().any(|p| stem.starts_with(p))
    })
}

fn program_command(program: &Path, file: &Path) -> Command {
    // An application bundle is started by Launch Services.
    if cfg!(target_os = "macos") && program.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")) {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-a").arg(program).arg(file);
        return c;
    }
    let mut c = Command::new(program);
    c.arg(file);
    c
}

/// The system's text editor: Notepad by its full path on Windows (see
/// `windows_notepad`), the default text editor on macOS (`open -t`). On
/// Linux, the application registered for text/plain is started on it
/// (`gtk-launch`), or GIO opens it by its `.txt` name; anything that would
/// pick the program from the file's content is not used.
fn system_editor(path: &Path) -> Result<Command, String> {
    #[cfg(windows)]
    {
        let notepad = windows_notepad()
            .ok_or_else(|| "Notepad was not found: choose an editor in Settings, or download the file".to_owned())?;
        Ok(program_command(&notepad, path))
    }
    #[cfg(target_os = "macos")]
    {
        let mut c = Command::new("/usr/bin/open");
        c.arg("-t").arg(path);
        Ok(c)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    linux_text_editor(path).ok_or_else(|| {
        "no text editor found (gtk-launch or gio is needed): choose an editor in Settings, or download the file"
            .to_owned()
    })
}

/// Notepad's full path: the classic one in the Windows folder, or, where only
/// the Store version is installed (recent Windows 11), its app alias.
#[cfg(windows)]
fn windows_notepad() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()));
    let alias = std::env::var_os("LOCALAPPDATA").map(|d| {
        PathBuf::from(d)
            .join("Microsoft")
            .join("WindowsApps")
            .join("notepad.exe")
    });
    [
        Some(root.join("System32").join("notepad.exe")),
        Some(root.join("notepad.exe")),
        alias,
    ]
    .into_iter()
    .flatten()
    .filter(|p| p.is_absolute())
    // An app alias is a reparse point that cannot be followed: check the
    // entry itself.
    .find(|p| std::fs::symlink_metadata(p).is_ok())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn linux_text_editor(path: &Path) -> Option<Command> {
    // The desktop's text/plain application, started explicitly.
    let handler = Command::new("xdg-mime")
        .args(["query", "default", "text/plain"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .filter(|id| {
            id.ends_with(".desktop")
                && id.len() < 256
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        });
    if let Some(id) = handler
        && on_path("gtk-launch")
    {
        let mut c = Command::new("gtk-launch");
        c.arg(id.trim_end_matches(".desktop")).arg(path);
        return Some(c);
    }
    // GIO picks the application from the name first: a .txt name is text.
    if on_path("gio") {
        let mut c = Command::new("gio");
        c.arg("open").arg(path);
        return Some(c);
    }
    None
}

#[cfg(all(unix, not(target_os = "macos")))]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choice_is_saved_and_reloaded() {
        let dir = tempfile::tempdir().unwrap();
        let editor = EditorChoice::load(dir.path());
        assert!(editor.info().is_none());
        let program = std::env::current_exe().unwrap();
        editor.set(Some(program.clone())).unwrap();
        let again = EditorChoice::load(dir.path());
        assert_eq!(again.info().unwrap().path, program.display().to_string());
        again.set(None).unwrap();
        assert!(EditorChoice::load(dir.path()).info().is_none());
    }

    #[test]
    fn broken_or_relative_choice_means_system_editor() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("editor.json"), b"{not json").unwrap();
        assert!(EditorChoice::load(dir.path()).info().is_none());
        std::fs::write(
            dir.path().join("editor.json"),
            br#"{"version":1,"program":"notepad.exe"}"#,
        )
        .unwrap();
        assert!(EditorChoice::load(dir.path()).info().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn only_executables_are_accepted() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("plain.txt");
        std::fs::write(&script, b"text").unwrap();
        assert!(check_program(&script).is_err(), "not executable");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_program(&script).is_ok());
        // Interpreters would run the file instead of opening it.
        let shell = dir.path().join("bash");
        std::fs::write(&shell, b"").unwrap();
        std::fs::set_permissions(&shell, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(check_program(&shell).is_err());
        let disguised = dir.path().join("myeditor");
        std::os::unix::fs::symlink(&shell, &disguised).unwrap();
        assert!(check_program(&disguised).is_err(), "a link to an interpreter");
        assert!(check_program(dir.path()).is_err(), "a folder");
        assert!(check_program(Path::new("relative/editor")).is_err());
        assert!(check_program(&dir.path().join("missing")).is_err());
    }
}
