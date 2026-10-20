//! The desktop around Termdock: a web link from a terminal opened in the
//! browser, and system notifications. Both go through here so the backend
//! checks what the UI asks for; neither plugin's own commands are allowed.

use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use termdock_core::sftp::is_format_char;
use url::Url;

use crate::commands::{CmdError, CmdResult};

/// Longer links are not opened (browsers and shells cope badly with them).
const MAX_URL: usize = 2048;
/// Notification texts are cut to this many characters.
const MAX_NOTE: usize = 120;

/// The link as it will be opened: `http` or `https` with a host, without a
/// user name or password (`https://bank.example@evil.example` hides where it
/// goes), normalised (host in punycode, spaces encoded).
pub fn web_url(raw: &str) -> Result<String, &'static str> {
    if raw.len() > MAX_URL {
        return Err("the link is too long");
    }
    if raw.chars().any(|c| c.is_control() || is_format_char(c)) {
        return Err("the link contains control or invisible characters");
    }
    let url = Url::parse(raw.trim()).map_err(|_| "not a web link")?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err("only http and https links are opened");
    }
    if url.host_str().is_none_or(str::is_empty) {
        return Err("the link has no host");
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("links with a user name or password are not opened");
    }
    Ok(url.into())
}

/// Opens a web link the user clicked (and confirmed) in the default browser.
#[tauri::command(async)]
pub fn open_url(app: AppHandle, url: String) -> CmdResult<()> {
    let url = web_url(&url).map_err(CmdError::msg)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| CmdError::msg(e.to_string()))
}

/// One line of plain text: no control or invisible characters, at most
/// [`MAX_NOTE`] characters (the text may come from a server).
fn note_text(text: &str) -> String {
    let clean: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .filter(|c| !is_format_char(*c))
        .collect();
    let mut words = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.chars().count() > MAX_NOTE {
        words = words.chars().take(MAX_NOTE - 1).collect::<String>() + "…";
    }
    words
}

/// Shows a system notification (a bell rang in a terminal the user is not
/// looking at).
#[tauri::command(async)]
pub fn notify(app: AppHandle, title: String, body: String) -> CmdResult<()> {
    app.notification()
        .builder()
        .title(note_text(&title))
        .body(note_text(&body))
        .show()
        .map_err(|e| CmdError::msg(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_plain_web_links_are_opened() {
        assert_eq!(
            web_url("https://example.com/a b?q=1").unwrap(),
            "https://example.com/a%20b?q=1"
        );
        assert_eq!(web_url("http://Example.COM").unwrap(), "http://example.com/");
        assert_eq!(
            web_url("https://bücher.example/").unwrap(),
            "https://xn--bcher-kva.example/"
        );
        for bad in [
            "file:///etc/passwd",
            "javascript:alert(1)",
            "ssh://host",
            "smb://server/share",
            "ms-settings:privacy",
            "https://user:pass@example.com/",
            "https://bank.example@evil.example/",
            "https://exa\u{202e}mple.com/",
            "https://example.com/\n",
            "not a url",
            "https://",
        ] {
            assert!(web_url(bad).is_err(), "{bad:?}");
        }
        assert!(web_url(&format!("https://example.com/{}", "a".repeat(MAX_URL))).is_err());
    }

    #[test]
    fn notification_text_is_one_short_line() {
        assert_eq!(note_text("a\nb\x1b[31m\u{202e}c"), "a b [31mc");
        assert_eq!(note_text(&"x".repeat(500)).chars().count(), MAX_NOTE);
    }
}
