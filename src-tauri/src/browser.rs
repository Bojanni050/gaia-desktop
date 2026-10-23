//! Opening a web address in the person's own browser.
//!
//! This is the desktop's own capability, not Desktop Commander's: it needs no
//! tool server and no shell. Windows' shell handler is asked to open the address
//! with whatever the person has set as their default browser, and the desktop
//! reports back whether that worked.
//!
//! What may be opened is deliberately narrow: an ordinary `http://` or
//! `https://` address and nothing else. That is checked twice — Gaia Cloud's
//! `desktopBroker.openableUrl` refuses anything else before an action is even
//! created, and this module checks again, because this is the step that hands a
//! string that came from a model to the operating system. `file:`, `javascript:`,
//! `data:` and app links are not web pages; they are local operations dressed up
//! as one.

use std::process::{Command, Stdio};

use url::Url;

use crate::error::DesktopError;

/// Longer than any real address, and short enough that nothing surprising fits.
const MAX_URL_CHARS: usize = 2048;

/// The address to open, normalised, or `None` when this is not a plain web address.
pub fn openable(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_URL_CHARS {
        return None;
    }
    // Control characters (a newline, a bell) never belong in an address; the URL
    // parser would silently strip some of them, which is exactly how a check gets
    // talked around.
    if trimmed.chars().any(char::is_control) {
        return None;
    }
    let parsed = Url::parse(trimmed).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    match parsed.host_str() {
        Some(host) if !host.is_empty() => Some(parsed.to_string()),
        _ => None,
    }
}

/// Hands the address to the shell's own handler. `rundll32 url.dll,FileProtocolHandler`
/// is how Windows opens "whatever handles this" without going through `cmd.exe`, so the
/// address stays one argument and nothing in it can be read as a command.
fn launch(href: &str) -> Result<(), DesktopError> {
    let mut command = Command::new("rundll32");
    command
        .args(["url.dll,FileProtocolHandler", href])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
    }
    command.spawn().map(|_| ()).map_err(|problem| {
        DesktopError::Message(format!("could not ask Windows to open the address: {problem}"))
    })
}

/// Opens a web address in the person's browser. Rejects anything that is not an
/// ordinary `http(s)` address rather than trying to make it work.
#[tauri::command]
pub fn browser_open_url(url: String) -> Result<(), DesktopError> {
    let href = openable(&url).ok_or_else(|| {
        DesktopError::Message(
            "only ordinary web addresses can be opened (a full http:// or https:// address, without a username or password in it)".to_string(),
        )
    })?;
    launch(&href)
}

#[cfg(test)]
mod tests {
    use super::openable;

    #[test]
    fn ordinary_addresses_are_accepted_and_normalised() {
        assert_eq!(openable("https://nu.nl").as_deref(), Some("https://nu.nl/"));
        assert_eq!(openable("  http://example.com/path  ").as_deref(), Some("http://example.com/path"));
        assert_eq!(openable("https://example.com/?q=a b").as_deref(), Some("https://example.com/?q=a%20b"));
    }

    #[test]
    fn anything_that_is_not_a_plain_web_address_is_refused() {
        for refused in [
            "javascript:alert(1)",
            "file:///C:/secret.txt",
            "ms-settings:privacy",
            "data:text/html,hi",
            "nu.nl",
            "https://user:pw@example.com",
            "https://exa mple.com",
            "https://example.com/\u{0007}",
            "",
            "   ",
        ] {
            assert_eq!(openable(refused), None, "should have been refused: {refused}");
        }
    }
}
