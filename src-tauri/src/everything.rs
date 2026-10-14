//! Instant file and folder search through Everything (voidtools).
//!
//! Everything keeps a live index of every drive; its command-line client `es.exe` asks it.
//! This is the one search that finds *folders* by name, and it answers in a fraction of a second
//! where walking a drive takes tens of seconds. The desktop only runs the query and hands back
//! paths; which of them Gaia may see is decided by the caller against the allowed folders.
//!
//! `es.exe` is started directly, never through a shell, and the query is always one argument, so
//! nothing a query contains can become a command.

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use tauri::Manager;

use crate::error::DesktopError;

const RUN_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_RESULTS_CAP: usize = 200;

/// What kind of entry to return.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Any,
    Files,
    Folders,
}

impl Kind {
    fn parse(text: Option<&str>) -> Kind {
        match text.map(|t| t.trim().to_ascii_lowercase()).as_deref() {
            Some("folders") | Some("folder") | Some("dirs") => Kind::Folders,
            Some("files") | Some("file") => Kind::Files,
            _ => Kind::Any,
        }
    }
}

/// The `es.exe` arguments for one query, or why the query is refused.
///
/// A query that starts with `-` or `/` would be read as an `es.exe` switch instead of a search
/// term, so it is refused rather than guessed at.
pub fn build_args(
    query: &str,
    kind: Kind,
    path: Option<&str>,
    max: usize,
) -> Result<Vec<String>, String> {
    let query: String = query.chars().filter(|c| !c.is_control()).collect();
    let query = query.trim();
    if query.is_empty() {
        return Err("empty search query".to_string());
    }
    if query.starts_with('-') || query.starts_with('/') {
        return Err("a search query cannot start with - or /".to_string());
    }
    let mut args = vec![
        "-n".to_string(),
        max.clamp(1, MAX_RESULTS_CAP).to_string(),
        "-timeout".to_string(),
        "8000".to_string(),
    ];
    match kind {
        Kind::Folders => args.push("/ad".to_string()),
        Kind::Files => args.push("/a-d".to_string()),
        Kind::Any => {}
    }
    if let Some(path) = path.map(str::trim).filter(|p| !p.is_empty()) {
        if path.chars().any(char::is_control) {
            return Err("invalid search path".to_string());
        }
        args.push("-path".to_string());
        args.push(path.to_string());
    }
    args.push(query.to_string());
    Ok(args)
}

/// One path per non-empty line. `es.exe` writes UTF-8 when its output is piped.
pub fn parse_output(bytes: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| l.trim_end_matches('\r').trim())
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// `es.exe` in the app's own tools folder first, then on PATH.
fn locate(app: &tauri::AppHandle) -> Option<PathBuf> {
    if let Ok(dir) = app.path().app_data_dir() {
        let own = dir.join("tools").join("es.exe");
        if own.is_file() {
            return Some(own);
        }
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("es.exe"))
        .find(|candidate| candidate.is_file())
}

fn run(es: &PathBuf, args: &[String]) -> Result<Vec<String>, String> {
    let mut command = Command::new(es);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
    }
    let mut child = command.spawn().map_err(|e| format!("could not start es.exe: {e}"))?;
    let mut stdout = child.stdout.take().ok_or("es.exe has no output")?;
    let mut stderr = child.stderr.take().ok_or("es.exe has no error output")?;

    // Read on threads so the timeout holds even if es.exe never finishes.
    let (tx, rx) = mpsc::channel();
    let out_tx = tx.clone();
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        let _ = out_tx.send(("out", buffer));
    });
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        let _ = tx.send(("err", buffer));
    });
    let (mut out, mut err) = (None, None);
    for _ in 0..2 {
        match rx.recv_timeout(RUN_TIMEOUT) {
            Ok(("out", bytes)) => out = Some(bytes),
            Ok((_, bytes)) => err = Some(bytes),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("es.exe did not answer in time".to_string());
            }
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        let message = String::from_utf8_lossy(&err.unwrap_or_default()).trim().to_string();
        return Err(if message.is_empty() {
            format!("es.exe failed ({status}); is Everything running?")
        } else {
            message
        });
    }
    Ok(parse_output(&out.unwrap_or_default()))
}

/// Whether Everything can be asked right now: `es.exe` is there and Everything answers.
#[tauri::command]
pub async fn everything_available(app: tauri::AppHandle) -> bool {
    let Some(es) = locate(&app) else { return false };
    tauri::async_runtime::spawn_blocking(move || {
        build_args("a", Kind::Any, None, 1)
            .map(|args| run(&es, &args).is_ok())
            .unwrap_or(false)
    })
    .await
    .unwrap_or(false)
}

/// Search the index. `kind` is `files`, `folders` or anything else for both; `path` limits the
/// search to one folder and below.
#[tauri::command]
pub async fn everything_search(
    app: tauri::AppHandle,
    query: String,
    kind: Option<String>,
    path: Option<String>,
    max: Option<usize>,
) -> Result<Vec<String>, DesktopError> {
    let es = locate(&app)
        .ok_or_else(|| DesktopError::Message("es.exe (Everything) is not installed".to_string()))?;
    let args = build_args(&query, Kind::parse(kind.as_deref()), path.as_deref(), max.unwrap_or(50))
        .map_err(DesktopError::Message)?;
    tauri::async_runtime::spawn_blocking(move || run(&es, &args))
        .await
        .map_err(|e| DesktopError::Message(e.to_string()))?
        .map_err(DesktopError::Message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_arguments_with_the_query_last_and_as_one_argument() {
        let args = build_args("gaia token", Kind::Files, Some("D:\\OneDrive"), 20).unwrap();
        assert_eq!(
            args,
            ["-n", "20", "-timeout", "8000", "/a-d", "-path", "D:\\OneDrive", "gaia token"]
        );
        let folders = build_args("txtjes", Kind::Folders, None, 5).unwrap();
        assert!(folders.contains(&"/ad".to_string()));
        assert_eq!(folders.last().map(String::as_str), Some("txtjes"));
        assert!(!build_args("x", Kind::Any, None, 5).unwrap().contains(&"-path".to_string()));
    }

    #[test]
    fn a_query_can_never_be_read_as_a_switch() {
        for bad in ["-path C:\\", "/ad", "", "   ", "\u{7}"] {
            assert!(build_args(bad, Kind::Any, None, 5).is_err(), "{bad:?}");
        }
        // inner dashes and slashes are ordinary text
        assert!(build_args("my-file/notes", Kind::Any, None, 5).is_ok());
        // control characters are stripped, not passed on
        let args = build_args("a\nb", Kind::Any, None, 5).unwrap();
        assert_eq!(args.last().map(String::as_str), Some("ab"));
        assert!(build_args("x", Kind::Any, Some("C:\\a\nb"), 5).is_err());
    }

    #[test]
    fn the_result_count_is_clamped() {
        let count = |max| build_args("x", Kind::Any, None, max).unwrap()[1].clone();
        assert_eq!(count(0), "1");
        assert_eq!(count(50), "50");
        assert_eq!(count(100_000), MAX_RESULTS_CAP.to_string());
    }

    #[test]
    fn output_is_one_path_per_line_in_utf8() {
        let text = "D:\\a\\ö結.txt\r\n\r\n  D:\\b  \r\n";
        assert_eq!(parse_output(text.as_bytes()), ["D:\\a\\ö結.txt", "D:\\b"]);
        assert!(parse_output(b"").is_empty());
    }

    #[test]
    fn kind_parses_loosely() {
        assert_eq!(Kind::parse(Some("Folders")), Kind::Folders);
        assert_eq!(Kind::parse(Some(" files ")), Kind::Files);
        assert_eq!(Kind::parse(Some("whatever")), Kind::Any);
        assert_eq!(Kind::parse(None), Kind::Any);
    }
}
