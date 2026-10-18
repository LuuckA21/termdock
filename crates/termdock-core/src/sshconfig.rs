//! Import of connections from an OpenSSH client configuration (`~/.ssh/config`).
//!
//! Supported: `Host` blocks with OpenSSH's first-value-wins semantics
//! (including values inherited from wildcard blocks such as `Host *`),
//! `HostName`, `User`, `Port`, `IdentityFile`, `ProxyJump` (one hop that is
//! itself a `Host` in the file), `LocalForward`, `RemoteForward`,
//! `DynamicForward` and `ServerAliveInterval`. Anything Termdock cannot
//! reproduce safely (`ProxyCommand`, `Match`, `Include`, Unix socket forwards…)
//! is reported as a warning instead of being guessed.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::Serialize;
use uuid::Uuid;

use crate::profile::{DEFAULT_SSH_PORT, Forward, Profile, ProfileKind, SshAuth, SshTarget};

pub const IMPORT_GROUP: &str = "SSH config";
const MAX_FILE_SIZE: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Candidate {
    pub alias: String,
    pub profile: Profile,
    /// `ProxyJump` alias, resolved to a profile at import time.
    pub jump_alias: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub candidates: Vec<Candidate>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
struct Block {
    /// `None` for options before the first `Host` (they apply to every host).
    patterns: Option<Vec<String>>,
    options: Vec<(String, Vec<String>)>,
}

/// Splits a value into arguments, honouring double quotes.
fn split_args(value: &str) -> Vec<String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut has = false;
    for c in value.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                has = true;
            }
            c if c.is_whitespace() && !quoted => {
                if has {
                    args.push(std::mem::take(&mut cur));
                    has = false;
                }
            }
            c => {
                cur.push(c);
                has = true;
            }
        }
    }
    if has {
        args.push(cur);
    }
    args
}

fn parse_blocks(text: &str, warnings: &mut Vec<String>) -> Vec<Block> {
    let mut blocks = vec![Block {
        patterns: None,
        options: Vec::new(),
    }];
    let mut in_match = false;
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let split = line.find(|c: char| c.is_whitespace() || c == '=').unwrap_or(line.len());
        let keyword = line[..split].to_ascii_lowercase();
        let rest = line[split..].trim_start();
        let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
        let args = split_args(rest);
        match keyword.as_str() {
            "host" => {
                in_match = false;
                blocks.push(Block {
                    patterns: Some(args),
                    options: Vec::new(),
                });
            }
            "match" => {
                in_match = true;
                warnings.push(format!(
                    "line {}: Match blocks are not supported and were skipped",
                    n + 1
                ));
            }
            "include" => warnings.push(format!("line {}: Include is not supported ({rest})", n + 1)),
            _ if in_match => {}
            _ if !args.is_empty() => blocks.last_mut().unwrap().options.push((keyword, args)),
            _ => {}
        }
    }
    blocks
}

fn wildcard(pattern: &str, text: &str) -> bool {
    crate::hostkeys::wildcard_match(&pattern.to_lowercase(), &text.to_lowercase())
}

fn block_matches(patterns: &Option<Vec<String>>, alias: &str) -> bool {
    let Some(patterns) = patterns else { return true };
    let mut hit = false;
    for p in patterns {
        if let Some(neg) = p.strip_prefix('!') {
            if wildcard(neg, alias) {
                return false;
            }
        } else if wildcard(p, alias) {
            hit = true;
        }
    }
    hit
}

const MULTI: &[&str] = &["identityfile", "localforward", "remoteforward", "dynamicforward"];

/// Effective options for `alias`: first value wins, except for the options
/// that OpenSSH accumulates.
fn options_for(blocks: &[Block], alias: &str) -> HashMap<String, Vec<Vec<String>>> {
    let mut out: HashMap<String, Vec<Vec<String>>> = HashMap::new();
    for b in blocks.iter().filter(|b| block_matches(&b.patterns, alias)) {
        for (k, v) in &b.options {
            let entry = out.entry(k.clone()).or_default();
            if entry.is_empty() || MULTI.contains(&k.as_str()) {
                entry.push(v.clone());
            }
        }
    }
    out
}

/// `[host:]port` with optional `[v6]` brackets.
fn split_host_port(s: &str, default_host: &str) -> Option<(String, u16)> {
    if let Some(v6) = s.strip_prefix('[') {
        let (h, rest) = v6.split_once(']')?;
        return Some((h.to_owned(), rest.strip_prefix(':')?.parse().ok()?));
    }
    match s.rsplit_once(':') {
        Some((h, p)) if !h.contains(':') => Some((if h.is_empty() { "*" } else { h }.to_owned(), p.parse().ok()?)),
        Some(_) => None,
        None => Some((default_host.to_owned(), s.parse().ok()?)),
    }
}

fn expand_tokens(value: &str, home: &Path, alias: &str, host: &str, user: &str, local_user: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next()? {
            '%' => out.push('%'),
            'd' => out.push_str(&home.to_string_lossy()),
            'h' => out.push_str(host),
            'n' => out.push_str(alias),
            'r' => out.push_str(user),
            'u' => out.push_str(local_user),
            _ => return None,
        }
    }
    if let Some(rest) = out.strip_prefix("~/") {
        return Some(home.join(rest).to_string_lossy().into_owned());
    }
    Some(out)
}

pub fn parse(text: &str, home: &Path, local_user: &str) -> Parsed {
    let mut parsed = Parsed::default();
    if text.len() > MAX_FILE_SIZE {
        parsed.warnings.push("file too large".into());
        return parsed;
    }
    let blocks = parse_blocks(text, &mut parsed.warnings);

    let mut seen = HashSet::new();
    let aliases: Vec<String> = blocks
        .iter()
        .filter_map(|b| b.patterns.as_ref())
        .flatten()
        .filter(|p| !p.contains(['*', '?']) && !p.starts_with('!'))
        .filter(|p| seen.insert(p.to_lowercase()))
        .cloned()
        .collect();

    for alias in aliases {
        let opts = options_for(&blocks, &alias);
        let first = |k: &str| opts.get(k).and_then(|v| v.first()).and_then(|a| a.first()).cloned();
        let mut warnings = Vec::new();

        let host = first("hostname")
            .map(|h| h.replace("%h", &alias).replace("%%", "%"))
            .unwrap_or_else(|| alias.clone());
        let username = first("user").unwrap_or_else(|| local_user.to_owned());
        let port = match first("port").map(|p| p.parse::<u16>()) {
            None => DEFAULT_SSH_PORT,
            Some(Ok(p)) if p > 0 => p,
            Some(_) => {
                parsed.warnings.push(format!("{alias}: invalid Port, skipped"));
                continue;
            }
        };
        let auth = match first("identityfile") {
            Some(f) => match expand_tokens(&f, home, &alias, &host, &username, local_user) {
                Some(path) => SshAuth::Key {
                    path: PathBuf::from(path),
                },
                None => {
                    warnings.push(format!(
                        "IdentityFile {f} uses unsupported tokens; using automatic authentication"
                    ));
                    SshAuth::Auto
                }
            },
            None => SshAuth::Auto,
        };
        if opts.get("identityfile").is_some_and(|v| v.len() > 1) {
            warnings.push("only the first IdentityFile is used".into());
        }
        let keepalive_secs = first("serveraliveinterval")
            .and_then(|v| v.parse::<u32>().ok())
            .map(|v| v.min(3600))
            .unwrap_or(30);

        let jump_alias = match first("proxyjump") {
            Some(j) if j.eq_ignore_ascii_case("none") => None,
            Some(j) if j.contains(',') => {
                warnings.push(format!(
                    "ProxyJump {j}: chains are imported only via each Host's own ProxyJump"
                ));
                None
            }
            Some(j) if j.contains(['@', ':']) => {
                warnings.push(format!("ProxyJump {j}: only aliases defined in the file are imported"));
                None
            }
            other => other,
        };
        if first("proxycommand").is_some_and(|c| !c.eq_ignore_ascii_case("none")) {
            warnings.push("ProxyCommand is not supported (use a jump host)".into());
        }

        let mut forwards = Vec::new();
        for (kind, args) in ["localforward", "remoteforward", "dynamicforward"]
            .iter()
            .flat_map(|k| opts.get(*k).into_iter().flatten().map(move |a| (*k, a)))
        {
            let raw = args.join(" ");
            let fwd = match (kind, args.as_slice()) {
                (_, [a, ..]) if a.contains('/') => None,
                ("dynamicforward", [bind]) => split_host_port(bind, "127.0.0.1").map(|(h, p)| Forward::Dynamic {
                    bind_host: if h == "*" { "0.0.0.0".into() } else { h },
                    bind_port: p,
                }),
                ("localforward", [bind, dest]) => split_host_port(bind, "127.0.0.1")
                    .zip(split_host_port(dest, ""))
                    .filter(|(_, (dh, _))| !dh.is_empty())
                    .map(|((bh, bp), (dh, dp))| Forward::Local {
                        bind_host: if bh == "*" { "0.0.0.0".into() } else { bh },
                        bind_port: bp,
                        dest_host: dh,
                        dest_port: dp,
                    }),
                ("remoteforward", [bind, dest]) => split_host_port(bind, "localhost")
                    .zip(split_host_port(dest, ""))
                    .filter(|(_, (dh, _))| !dh.is_empty())
                    .map(|((bh, bp), (dh, dp))| Forward::Remote {
                        bind_host: if bh == "*" { "0.0.0.0".into() } else { bh },
                        bind_port: bp,
                        dest_host: dh,
                        dest_port: dp,
                    }),
                _ => None,
            };
            match fwd {
                Some(f) => forwards.push(f),
                None => warnings.push(format!("{kind} {raw}: not supported, skipped")),
            }
        }

        let mut profile = Profile::new(
            alias.clone(),
            ProfileKind::Ssh(SshTarget {
                host,
                port,
                username,
                auth,
                keepalive_secs,
                jump: None,
                forwards,
            }),
        );
        profile.group = Some(IMPORT_GROUP.into());
        if let Err(e) = profile.validate() {
            parsed.warnings.push(format!("{alias}: {e}, skipped"));
            continue;
        }
        parsed.candidates.push(Candidate {
            alias,
            profile,
            jump_alias,
            warnings,
        });
    }
    parsed
}

/// What the import dialog shows for one host.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewItem {
    pub alias: String,
    pub target: String,
    pub jump: Option<String>,
    pub forwards: Vec<String>,
    pub warnings: Vec<String>,
    /// Name of a saved connection with the same user, host and port.
    pub existing: Option<String>,
}

fn same_endpoint(a: &SshTarget, b: &SshTarget) -> bool {
    a.host.eq_ignore_ascii_case(&b.host) && a.port == b.port && a.username == b.username
}

fn ssh_target(p: &Profile) -> Option<&SshTarget> {
    match &p.kind {
        ProfileKind::Ssh(t) => Some(t),
        ProfileKind::Local(_) => None,
    }
}

fn existing_for<'a>(existing: &'a [Profile], candidate: &Candidate) -> Option<&'a Profile> {
    let t = ssh_target(&candidate.profile)?;
    existing
        .iter()
        .find(|p| ssh_target(p).is_some_and(|e| same_endpoint(e, t)))
}

pub fn preview(parsed: &Parsed, existing: &[Profile]) -> Vec<PreviewItem> {
    parsed
        .candidates
        .iter()
        .map(|c| {
            let t = ssh_target(&c.profile).expect("candidates are SSH");
            PreviewItem {
                alias: c.alias.clone(),
                target: t.display(),
                jump: c.jump_alias.clone(),
                forwards: t.forwards.iter().map(Forward::describe).collect(),
                warnings: c.warnings.clone(),
                existing: existing_for(existing, c).map(|p| p.name.clone()),
            }
        })
        .collect()
}

/// Profiles to create for the `selected` aliases. Hosts already saved are not
/// duplicated; jump hosts are linked to the new or the existing profile, and
/// a jump host that is neither selected nor saved is imported too, since the
/// connection would not work without it.
pub fn plan_import(parsed: &Parsed, existing: &[Profile], selected: &[String]) -> (Vec<Profile>, Vec<String>) {
    let by_alias: HashMap<String, &Candidate> = parsed.candidates.iter().map(|c| (c.alias.to_lowercase(), c)).collect();
    let mut wanted: Vec<&Candidate> = Vec::new();
    let mut queue: Vec<String> = selected.iter().map(|a| a.to_lowercase()).collect();
    let mut seen = HashSet::new();
    while let Some(a) = queue.pop() {
        if !seen.insert(a.clone()) {
            continue;
        }
        if let Some(c) = by_alias.get(&a) {
            wanted.push(c);
            if let Some(j) = &c.jump_alias {
                queue.push(j.to_lowercase());
            }
        }
    }

    let mut ids: HashMap<String, Uuid> = HashMap::new();
    let mut new_profiles: Vec<Profile> = Vec::new();
    let mut warnings = Vec::new();
    for c in &wanted {
        match existing_for(existing, c) {
            Some(p) => {
                ids.insert(c.alias.to_lowercase(), p.id);
            }
            None => {
                ids.insert(c.alias.to_lowercase(), c.profile.id);
                new_profiles.push(c.profile.clone());
            }
        }
    }
    for p in &mut new_profiles {
        let c = wanted.iter().find(|c| c.profile.id == p.id).expect("from wanted");
        if let (Some(j), ProfileKind::Ssh(t)) = (&c.jump_alias, &mut p.kind) {
            match ids.get(&j.to_lowercase()) {
                Some(id) if *id != p.id => t.jump = Some(*id),
                _ => warnings.push(format!(
                    "{}: ProxyJump {j} is not a Host in the file, not linked",
                    c.alias
                )),
            }
        }
    }
    // Restore the user's order.
    new_profiles.sort_by_key(|p| {
        parsed
            .candidates
            .iter()
            .position(|c| c.profile.id == p.id)
            .unwrap_or(usize::MAX)
    });
    // Then each jump host before the connections that use it: the store only
    // accepts a jump host that is already saved. A loop of ProxyJumps is cut.
    let mut ordered: Vec<Profile> = Vec::with_capacity(new_profiles.len());
    let mut pending = new_profiles;
    while !pending.is_empty() {
        let ready = pending.iter().position(|p| match &p.kind {
            ProfileKind::Ssh(t) => t.jump.is_none_or(|j| !pending.iter().any(|q| q.id == j)),
            _ => true,
        });
        let mut next = pending.remove(ready.unwrap_or(0));
        if ready.is_none()
            && let ProfileKind::Ssh(t) = &mut next.kind
        {
            t.jump = None;
            warnings.push(format!("{}: ProxyJump loop, not linked", next.name));
        }
        ordered.push(next);
    }
    (ordered, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jump_hosts_come_first_and_loops_are_cut() {
        let text = "Host web\n  HostName web.example.com\n  ProxyJump bastion\n\nHost bastion\n  HostName b.example.com\n\nHost a\n  HostName a.example.com\n  ProxyJump b\n\nHost b\n  HostName b2.example.com\n  ProxyJump a\n";
        let parsed = parse(text, Path::new("/home/me"), "me");
        let (profiles, warnings) = plan_import(&parsed, &[], &["web".into(), "a".into()]);
        let pos = |name: &str| profiles.iter().position(|p| p.name == name).unwrap();
        assert!(pos("bastion") < pos("web"));
        assert!(warnings.iter().any(|w| w.contains("ProxyJump loop")));
        // Saved in this order, every jump host exists before it is used.
        let dir = tempfile::tempdir().unwrap();
        let mut store = crate::store::ProfileStore::open(dir.path().join("p.json")).unwrap();
        for p in profiles {
            store.upsert(p).unwrap();
        }
    }

    const CONFIG: &str = r#"
# global defaults
ServerAliveInterval 15

Host bastion
    HostName bastion.example.com
    User jump
    IdentityFile ~/.ssh/id_ed25519

Host db db-alias
    HostName 10.0.4.12
    Port=2200
    ProxyJump bastion
    LocalForward 5433 localhost:5432
    LocalForward 127.0.0.1:8080 [fe80::1]:80
    DynamicForward 1080
    RemoteForward 9000 127.0.0.1:3000
    RemoteForward /tmp/sock /tmp/other
    IdentityFile "%d/.ssh/db key"

Host web-* !web-legacy
    User deploy

Host web-1 web-legacy
    HostName %h.internal

Host proxied
    ProxyCommand nc -X 5 -x proxy:1080 %h %p
    ProxyJump user@elsewhere:2222

Host plain
    ProxyCommand none

Match host foo
    User ignored

Host *
    User fallback
    Port 22
"#;

    fn parsed() -> Parsed {
        parse(CONFIG, Path::new("/home/me"), "me")
    }

    fn target<'a>(p: &'a Parsed, alias: &str) -> &'a SshTarget {
        let c = p.candidates.iter().find(|c| c.alias == alias).unwrap();
        ssh_target(&c.profile).unwrap()
    }

    #[test]
    fn hosts_and_inheritance() {
        let p = parsed();
        let aliases: Vec<_> = p.candidates.iter().map(|c| c.alias.as_str()).collect();
        assert_eq!(
            aliases,
            ["bastion", "db", "db-alias", "web-1", "web-legacy", "proxied", "plain"]
        );
        let plain = p.candidates.iter().find(|c| c.alias == "plain").unwrap();
        assert!(
            plain.warnings.is_empty(),
            "ProxyCommand none means no proxy: {:?}",
            plain.warnings
        );

        let b = target(&p, "bastion");
        assert_eq!(
            (b.host.as_str(), b.username.as_str(), b.port),
            ("bastion.example.com", "jump", 22)
        );
        assert_eq!(b.keepalive_secs, 15);
        assert_eq!(
            b.auth,
            SshAuth::Key {
                path: "/home/me/.ssh/id_ed25519".into()
            }
        );

        let db = target(&p, "db");
        assert_eq!((db.port, db.username.as_str()), (2200, "fallback"));
        assert_eq!(
            db.auth,
            SshAuth::Key {
                path: "/home/me/.ssh/db key".into()
            }
        );

        // Negated pattern: web-legacy does not get User deploy.
        assert_eq!(target(&p, "web-1").username, "deploy");
        assert_eq!(target(&p, "web-1").host, "web-1.internal");
        assert_eq!(target(&p, "web-legacy").username, "fallback");
        assert!(p.warnings.iter().any(|w| w.contains("Match")));
    }

    #[test]
    fn forwards_and_jumps() {
        let p = parsed();
        let db = p.candidates.iter().find(|c| c.alias == "db").unwrap();
        assert_eq!(db.jump_alias.as_deref(), Some("bastion"));
        let described: Vec<_> = target(&p, "db").forwards.iter().map(Forward::describe).collect();
        assert_eq!(
            described,
            [
                "L 127.0.0.1:5433 -> localhost:5432",
                "L 127.0.0.1:8080 -> [fe80::1]:80",
                "R localhost:9000 -> 127.0.0.1:3000",
                "D 127.0.0.1:1080 (SOCKS5)",
            ]
        );
        assert!(db.warnings.iter().any(|w| w.contains("/tmp/sock")));

        let proxied = p.candidates.iter().find(|c| c.alias == "proxied").unwrap();
        assert!(proxied.jump_alias.is_none());
        assert_eq!(proxied.warnings.len(), 2, "{:?}", proxied.warnings);
    }

    #[test]
    fn import_plan_links_jumps_and_skips_duplicates() {
        let p = parsed();
        // An identical endpoint for "bastion" is already saved.
        let mut saved = Profile::new(
            "my bastion",
            ProfileKind::Ssh(SshTarget {
                host: "BASTION.example.com".into(),
                port: 22,
                username: "jump".into(),
                auth: SshAuth::Agent,
                keepalive_secs: 30,
                jump: None,
                forwards: vec![],
            }),
        );
        saved.validate().unwrap();

        let items = preview(&p, std::slice::from_ref(&saved));
        assert_eq!(items[0].existing.as_deref(), Some("my bastion"));

        let (new, warnings) = plan_import(&p, std::slice::from_ref(&saved), &["db".into()]);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(new.len(), 1);
        assert_eq!(ssh_target(&new[0]).unwrap().jump, Some(saved.id));
        assert_eq!(new[0].group.as_deref(), Some(IMPORT_GROUP));

        // Without it, the bastion is imported along with db and linked.
        let (new, _) = plan_import(&p, &[], &["db".into()]);
        let names: Vec<_> = new.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["bastion", "db"]);
        assert_eq!(ssh_target(&new[1]).unwrap().jump, Some(new[0].id));
    }

    #[test]
    fn helpers() {
        assert_eq!(split_args(r#"a "b c" d"#), ["a", "b c", "d"]);
        assert_eq!(split_host_port("8080", "127.0.0.1"), Some(("127.0.0.1".into(), 8080)));
        assert_eq!(split_host_port("*:8080", "127.0.0.1"), Some(("*".into(), 8080)));
        assert_eq!(split_host_port("[::1]:22", ""), Some(("::1".into(), 22)));
        assert_eq!(split_host_port("::1:22", ""), None);
        assert!(wildcard("web-*", "WEB-1"));
        assert!(!wildcard("web-?", "web-10"));
        assert_eq!(
            expand_tokens("%d/k_%r@%h", Path::new("/h"), "a", "host", "root", "me").as_deref(),
            Some("/h/k_root@host")
        );
        assert_eq!(expand_tokens("%C", Path::new("/h"), "a", "h", "r", "me"), None);
        assert!(parse("Host x\n  Port 0\n", Path::new("/h"), "me").candidates.is_empty());
    }
}
