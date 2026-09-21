//! Chat-history export — downloads a saved conversation from Gaia Server's
//! `conversations/:id/export/:format` endpoint and writes it to a local
//! path chosen through a native save dialog. The response is a file download
//! (plain markdown text for `markdown`, JSON for `json`), not the opaque
//! JSON `server_request` carries, so this is its own small seam — the same
//! pattern the library uses for file bytes.

use crate::communication::ServerLink;
use crate::error::DesktopError;
use crate::library::{authorize, base_url, error_for_status};

/// Exports conversation `id` in `format` ("json" or "markdown") and writes
/// the bytes to `save_path` (already chosen via a native save dialog on
/// the frontend).
#[tauri::command]
pub async fn history_export_conversation(
    link: tauri::State<'_, ServerLink>,
    id: String,
    format: String,
    save_path: String,
) -> Result<(), DesktopError> {
    let base = base_url(&link)?;
    let client = reqwest::Client::new();
    let response = authorize(
        client.get(format!("{base}/conversations/{id}/export/{format}")),
        &link,
    )
    .send()
    .await
    .map_err(|e| DesktopError::Message(format!("could not reach Gaia Server: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| DesktopError::Message(format!("could not read export: {e}")))?;
    tokio::fs::write(&save_path, &bytes)
        .await
        .map_err(|e| DesktopError::Message(format!("could not write file: {e}")))?;
    Ok(())
}
