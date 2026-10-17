//! File library — transports bytes to/from Gaia Server's `/library` API.
//!
//! Server-side only: this module never stores a file locally, never
//! inspects file content, and never decides what belongs in the library
//! (architecture.md: no client holds canonical state — the library lives
//! in Gaia Cloud, same posture as everything else reached through
//! [`ServerLink`]). It isn't funneled through `server_request`'s generic
//! opaque-JSON contract because file bytes need multipart/binary
//! transport, which that seam doesn't carry — this is its own small,
//! purpose-built seam instead, the same pattern `capture`/`audio` already
//! use for concerns `server_request` doesn't fit.

use serde::{Deserialize, Serialize};

use crate::communication::{CommunicationError, ServerLink};
use crate::error::DesktopError;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryFile {
    pub id: String,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub uploaded_at: String,
}

pub fn base_url(link: &ServerLink) -> Result<String, DesktopError> {
    let config = link.config();
    let base = config
        .base_url
        .filter(|u| !u.trim().is_empty())
        .ok_or(CommunicationError::NotConfigured)?;
    Ok(base.trim_end_matches('/').to_string())
}

pub fn authorize(builder: reqwest::RequestBuilder, link: &ServerLink) -> reqwest::RequestBuilder {
    match link.config().auth_token {
        Some(token) => builder.bearer_auth(token),
        None => builder,
    }
}

pub async fn error_for_status(response: reqwest::Response) -> DesktopError {
    let status = response.status();
    DesktopError::Message(format!("Gaia Server responded with status {status}"))
}

/// The MIME type of `filename`, judged from its extension alone. The
/// client never inspects what is inside a file, and neither does the
/// server: the extension is all either of them needs to label it.
fn mime_for_filename(filename: &str) -> &'static str {
    match std::path::Path::new(filename)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "txt" | "text" => "text/plain",
        "json" => "application/json",
        "md" => "text/markdown",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" | "mjs" => "text/javascript",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "toml" => "text/plain",
        "yaml" | "yml" => "text/yaml",
        _ => "application/octet-stream",
    }
}

/// Sends `bytes` to Gaia Server as one multipart upload named `filename`.
/// The one place a library upload is actually performed — both callers
/// differ only in where the bytes came from.
async fn upload(
    link: &ServerLink,
    filename: String,
    mime_type: &str,
    bytes: Vec<u8>,
) -> Result<LibraryFile, DesktopError> {
    let base = base_url(link)?;
    let part = reqwest::multipart::Part::bytes(bytes)
        .file_name(filename)
        .mime_str(mime_type)
        .map_err(|e| DesktopError::Message(format!("invalid mime type: {e}")))?;
    let form = reqwest::multipart::Form::new().part("file", part);

    let client = reqwest::Client::new();
    let request = authorize(client.post(format!("{base}/library/files")), link).multipart(form);
    let response = request
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("upload failed: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }
    response
        .json::<LibraryFile>()
        .await
        .map_err(|e| DesktopError::Message(format!("unreadable response: {e}")))
}

/// Uploads the file at `path` (already chosen via a native file dialog on
/// the frontend) and returns the metadata Gaia Server stored it under.
#[tauri::command]
pub async fn library_upload_file(
    link: tauri::State<'_, ServerLink>,
    path: String,
) -> Result<LibraryFile, DesktopError> {
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|e| DesktopError::Message(format!("could not read file: {e}")))?;
    let filename = std::path::Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "upload".to_string());

    let mime_type = mime_for_filename(&filename);
    upload(&link, filename, mime_type, bytes).await
}

/// Uploads bytes the webview already holds — an image pasted into the
/// composer — without this device ever writing them to disk. Only the way
/// the bytes arrive differs from [`library_upload_file`]: they are the raw
/// request body here (a pasted screenshot is easily a few megabytes, and
/// raw is Tauri's fast path for that), with the file name in the
/// `x-gaia-filename` header; the MIME type is read from that name's
/// extension, exactly as above.
#[tauri::command]
pub async fn library_upload_pasted_file(
    link: tauri::State<'_, ServerLink>,
    request: tauri::ipc::Request<'_>,
) -> Result<LibraryFile, DesktopError> {
    let filename = request
        .headers()
        .get("x-gaia-filename")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DesktopError::Message("pasted file has no name (x-gaia-filename)".to_string()))?
        .to_string();

    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else {
        return Err(DesktopError::Message("pasted file did not arrive as bytes".to_string()));
    };

    let mime_type = mime_for_filename(&filename);
    upload(&link, filename, mime_type, bytes.clone()).await
}

/// Lists every file currently in the library.
#[tauri::command]
pub async fn library_list_files(
    link: tauri::State<'_, ServerLink>,
) -> Result<Vec<LibraryFile>, DesktopError> {
    let base = base_url(&link)?;
    let client = reqwest::Client::new();
    let response = authorize(client.get(format!("{base}/library/files")), &link)
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("could not reach Gaia Server: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }

    #[derive(Deserialize)]
    struct ListResponse {
        files: Vec<LibraryFile>,
    }
    let parsed: ListResponse = response
        .json()
        .await
        .map_err(|e| DesktopError::Message(format!("unreadable response: {e}")))?;
    Ok(parsed.files)
}

/// Downloads a file's bytes and writes them to `save_path` (already
/// chosen via a native save dialog on the frontend).
#[tauri::command]
pub async fn library_download_file(
    link: tauri::State<'_, ServerLink>,
    id: String,
    save_path: String,
) -> Result<(), DesktopError> {
    let base = base_url(&link)?;
    let client = reqwest::Client::new();
    let response = authorize(client.get(format!("{base}/library/files/{id}")), &link)
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("download failed: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }

    let bytes = response
        .bytes()
        .await
        .map_err(|e| DesktopError::Message(format!("could not read response: {e}")))?;
    tokio::fs::write(&save_path, &bytes)
        .await
        .map_err(|e| DesktopError::Message(format!("could not write file: {e}")))?;
    Ok(())
}

/// Deletes a file from the library.
#[tauri::command]
pub async fn library_delete_file(
    link: tauri::State<'_, ServerLink>,
    id: String,
) -> Result<(), DesktopError> {
    let base = base_url(&link)?;
    let client = reqwest::Client::new();
    let response = authorize(client.delete(format!("{base}/library/files/{id}")), &link)
        .send()
        .await
        .map_err(|e| DesktopError::Message(format!("delete failed: {e}")))?;

    if !response.status().is_success() {
        return Err(error_for_status(response).await);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::mime_for_filename;

    /// The extension is the whole of it: a pasted screenshot arrives as
    /// `pasted-image-….png` for exactly this reason, and a file whose
    /// extension nobody knows is labeled as opaque bytes rather than guessed at.
    #[test]
    fn mime_type_is_read_from_the_extension() {
        assert_eq!(mime_for_filename("pasted-image-20261018-143002.PNG"), "image/png");
        assert_eq!(mime_for_filename("foto.jpeg"), "image/jpeg");
        assert_eq!(mime_for_filename("rapport.pdf"), "application/pdf");
        assert_eq!(mime_for_filename("notities.md"), "text/markdown");
        assert_eq!(mime_for_filename("zonder-extensie"), "application/octet-stream");
    }
}
