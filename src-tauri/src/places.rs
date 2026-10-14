//! The person's well-known folders on this computer, resolved the way Windows
//! resolves them — so a Desktop that OneDrive moved to another drive is reported where it
//! really is. Told to Gaia so she can start looking in the right place instead of guessing.

use serde::Serialize;
use tauri::Manager;

use crate::error::DesktopError;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnownPlaces {
    pub home: Option<String>,
    pub desktop: Option<String>,
    pub documents: Option<String>,
    pub downloads: Option<String>,
}

fn text(path: Result<std::path::PathBuf, tauri::Error>) -> Option<String> {
    path.ok().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command]
pub fn desktop_known_places(app: tauri::AppHandle) -> Result<KnownPlaces, DesktopError> {
    let path = app.path();
    Ok(KnownPlaces {
        home: text(path.home_dir()),
        desktop: text(path.desktop_dir()),
        documents: text(path.document_dir()),
        downloads: text(path.download_dir()),
    })
}
