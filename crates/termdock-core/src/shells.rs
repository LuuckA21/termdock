//! Discovery of the local shells offered by the "new local terminal" menu.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::profile::LocalShell;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DetectedShell {
    pub id: String,
    pub name: String,
    pub shell: LocalShell,
    pub is_default: bool,
}

fn find_in_path(exe: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(exe))
        .find(|p| p.is_file())
}

fn shell(id: &str, name: &str, program: impl AsRef<Path>, args: &[&str]) -> DetectedShell {
    DetectedShell {
        id: id.into(),
        name: name.into(),
        shell: LocalShell {
            program: program.as_ref().to_string_lossy().into_owned(),
            args: args.iter().map(|s| (*s).to_owned()).collect(),
            cwd: None,
        },
        is_default: false,
    }
}

#[cfg(windows)]
pub fn detect() -> Vec<DetectedShell> {
    let mut out = Vec::new();
    if let Some(p) = find_in_path("pwsh.exe") {
        out.push(shell("pwsh", "PowerShell", p, &["-NoLogo"]));
    }
    if let Some(p) = find_in_path("powershell.exe") {
        out.push(shell("powershell", "Windows PowerShell", p, &["-NoLogo"]));
    }
    if let Some(p) = find_in_path("cmd.exe") {
        out.push(shell("cmd", "Command Prompt", p, &[]));
    }
    if let Some(p) = find_in_path("wsl.exe") {
        out.push(shell("wsl", "WSL", p, &[]));
    }
    for base in [std::env::var_os("ProgramFiles"), std::env::var_os("LOCALAPPDATA")]
        .into_iter()
        .flatten()
    {
        let bash = PathBuf::from(&base).join("Git").join("bin").join("bash.exe");
        let bash_user = PathBuf::from(&base)
            .join("Programs")
            .join("Git")
            .join("bin")
            .join("bash.exe");
        if let Some(p) = [bash, bash_user].into_iter().find(|p| p.is_file()) {
            out.push(shell("gitbash", "Git Bash", p, &["--login", "-i"]));
            break;
        }
    }
    if out.is_empty() {
        out.push(shell("cmd", "Command Prompt", "cmd.exe", &[]));
    }
    out[0].is_default = true;
    out
}

#[cfg(unix)]
pub fn detect() -> Vec<DetectedShell> {
    let login: &[&str] = if cfg!(target_os = "macos") { &["-l"] } else { &[] };
    let mut out = Vec::new();

    if let Some(sh) = std::env::var_os("SHELL").map(PathBuf::from).filter(|p| p.is_file()) {
        let name = sh
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut s = shell(&name, &name, &sh, login);
        s.is_default = true;
        out.push(s);
    }

    let listed = std::fs::read_to_string("/etc/shells").unwrap_or_default();
    let candidates = listed
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('/'))
        .map(PathBuf::from)
        .chain(
            ["bash", "zsh", "fish", "pwsh", "nu"]
                .iter()
                .filter_map(|e| find_in_path(e)),
        );

    for path in candidates {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        let wanted = matches!(name.as_str(), "bash" | "zsh" | "fish" | "pwsh" | "nu" | "sh");
        if wanted && path.is_file() && !out.iter().any(|s| s.id == name) {
            let display = if name == "pwsh" {
                "PowerShell".to_owned()
            } else {
                name.clone()
            };
            out.push(shell(&name, &display, &path, login));
        }
    }
    if out.is_empty() {
        out.push(shell("sh", "sh", "/bin/sh", &[]));
    }
    if !out.iter().any(|s| s.is_default) {
        out[0].is_default = true;
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn detects_at_least_one_default() {
        let shells = super::detect();
        assert!(!shells.is_empty());
        assert_eq!(shells.iter().filter(|s| s.is_default).count(), 1);
    }
}
