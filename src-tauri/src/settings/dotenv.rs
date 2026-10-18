//! Dev-only token seeding from `.env.local`.
//!
//! The Gaia Server auth token normally lives in the OS credential store (see
//! [`super::secrets`]). While working locally, a `GAIA_BRIDGE_TOKEN` in a
//! `.env.local` found *above the running executable* may fill an otherwise
//! empty token. That value stays in memory for the run: it is never written to
//! `settings.json` or the credential store — only an explicit save does that.

use std::path::{Path, PathBuf};

const ENV_FILE: &str = ".env.local";
const KEY: &str = "GAIA_BRIDGE_TOKEN";

/// The `GAIA_BRIDGE_TOKEN` from the nearest `.env.local` above the running
/// executable, or `None` when there is none. Works for both `target/debug`
/// (dev) and `target/release` (build), since both sit a few levels under the
/// repository root that holds the file.
pub fn bridge_token() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    token_above(exe.parent()?)
}

/// Walks `dir` and its ancestors and returns the first `GAIA_BRIDGE_TOKEN`
/// found in a `.env.local`.
fn token_above(dir: &Path) -> Option<String> {
    dir.ancestors()
        .map(|ancestor| ancestor.join(ENV_FILE))
        .find_map(|path| token_in(&path))
}

/// Reads one `.env.local`; a missing or unreadable file simply has no token.
fn token_in(path: &Path) -> Option<String> {
    parse(&std::fs::read_to_string(path).ok()?)
}

/// The `GAIA_BRIDGE_TOKEN` value in a `.env.local`'s text. Blank lines, `#`
/// comments, other keys and an empty value are all skipped, so a half-written
/// file never seeds a bogus token.
fn parse(raw: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != KEY {
            continue;
        }
        let value = value.trim().trim_matches(['"', '\'']);
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_token_from_a_plain_line() {
        assert_eq!(parse("GAIA_BRIDGE_TOKEN=abc123"), Some("abc123".to_string()));
    }

    #[test]
    fn ignores_comments_blanks_other_keys_and_empty_values() {
        let raw = "# GAIA_BRIDGE_TOKEN=comment\n\nOTHER=1\nGAIA_BRIDGE_TOKEN=\nGAIA_BRIDGE_TOKEN=real\n";
        assert_eq!(parse(raw), Some("real".to_string()));
        assert_eq!(parse("OTHER=1\n"), None);
        assert_eq!(parse(""), None);
    }

    #[test]
    fn strips_matching_quotes_and_surrounding_spaces() {
        assert_eq!(parse(" GAIA_BRIDGE_TOKEN = \"a b\" \n"), Some("a b".to_string()));
        assert_eq!(parse("GAIA_BRIDGE_TOKEN='quoted'"), Some("quoted".to_string()));
    }

    #[test]
    fn finds_a_token_file_in_a_parent_directory() {
        let root = std::env::temp_dir().join(format!("gaia-dotenv-{}", std::process::id()));
        let nested = root.join("src-tauri").join("target").join("debug");
        std::fs::create_dir_all(&nested).expect("temp tree");
        std::fs::write(root.join(ENV_FILE), "GAIA_BRIDGE_TOKEN=from-parent\n").expect("write env file");

        assert_eq!(token_above(&nested), Some("from-parent".to_string()));

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_nearest_file_wins_over_a_farther_one() {
        let root = std::env::temp_dir().join(format!("gaia-dotenv-near-{}", std::process::id()));
        let nested = root.join("a").join("b");
        std::fs::create_dir_all(&nested).expect("temp tree");
        std::fs::write(root.join(ENV_FILE), "GAIA_BRIDGE_TOKEN=far\n").expect("write far");
        std::fs::write(root.join("a").join(ENV_FILE), "GAIA_BRIDGE_TOKEN=near\n").expect("write near");

        assert_eq!(token_above(&nested), Some("near".to_string()));

        std::fs::remove_dir_all(&root).ok();
    }
}
