//! Secret storage — the OS credential store, not settings.json.
//!
//! The Gaia Server auth token is a per-device secret: it belongs in the
//! platform's credential store (Windows Credential Manager, macOS Keychain,
//! the session keyring on Linux), never on disk in plain JSON. On keyring
//! failure callers fall back to the previous in-file behaviour — no worse
//! than before, and settings still save.

use keyring::Entry;

const SERVICE: &str = "ai.gaia.desktop";
const ACCOUNT: &str = "server-auth-token";

/// Store (or, for `None`/empty, delete) the server auth token.
pub fn store_token(token: Option<&str>) -> Result<(), keyring::Error> {
    let entry = Entry::new(SERVICE, ACCOUNT)?;
    match token.map(str::trim).filter(|t| !t.is_empty()) {
        Some(token) => entry.set_password(token),
        None => match entry.get_password() {
            Ok(_) => entry.delete_credential(),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error),
        },
    }
}

/// Load the stored token, if any. `None` when nothing is stored or the
/// keyring is unavailable — callers treat that as "no token configured".
pub fn load_token() -> Option<String> {
    let entry = Entry::new(SERVICE, ACCOUNT).ok()?;
    entry.get_password().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Guards against the keyring mock-store trap: without the platform
    /// feature, `set_password` "succeeds" while storing nothing, which once
    /// silently destroyed the only copy of the token. A real store must
    /// round-trip.
    #[test]
    fn token_round_trips_through_the_real_store() {
        store_token(Some("round-trip-probe")).expect("store must succeed");
        assert_eq!(load_token().as_deref(), Some("round-trip-probe"));
        store_token(None).expect("delete must succeed");
        assert_eq!(load_token(), None);
    }
}
