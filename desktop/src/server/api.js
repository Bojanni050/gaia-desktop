/**
 * The desktop's single bridge to the Rust shell — and through it, to Gaia Cloud.
 *
 * Every call crosses the ServerLink seam in Rust. The UI never fetches a
 * backend directly and never embeds Gaia Web: this is a client of Gaia
 * Cloud's API, not a browser for another client.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open as openDialog, save as saveDialog } from '@tauri-apps/plugin-dialog';
import {
  buildHistoryListRequest,
  buildHistoryGetRequest,
  buildHistoryDeleteRequest,
  parseHistoryList,
  parseHistoryConversation,
} from '../state/contract';

export const serverApi = {
  getConfig: () => invoke('server_get_config'),
  applyConfig: (config) => invoke('server_set_config', { config }),
  getStatus: () => invoke('server_get_status'),
  testConnection: () => invoke('server_test_connection'),
  request: (request) => invoke('server_request', { request }),
  onStatus: (handler) => listen('server://status', (event) => handler(event.payload)),
};

export const presenceApi = {
  get: () => invoke('presence_get'),
  setQuiet: (quiet) => invoke('presence_set_quiet', { quiet }),
  onChanged: (handler) => listen('presence://changed', (event) => handler(event.payload)),
};

export const settingsApi = {
  get: () => invoke('settings_get'),
  save: (newSettings) => invoke('settings_save', { newSettings }),
};

export const captureApi = {
  listSources: () => invoke('capture_list_sources'),
};

export const audioApi = {
  getStatus: () => invoke('audio_get_status'),
};

/**
 * The file library. File bytes never cross the generic `server_request`
 * seam (JSON-only) — `library_upload_file`/`library_download_file` are
 * dedicated Rust commands that read/write local paths chosen through a
 * native dialog and stream the bytes to/from Gaia Server directly. This
 * module still never talks to Gaia Server itself; it only ever picks a
 * local path and hands it to Rust.
 */
export const libraryApi = {
  listFiles: () => invoke('library_list_files'),
  deleteFile: (id) => invoke('library_delete_file', { id }),
  /** Opens a native file picker; returns the uploaded file's metadata, or null if the picker was cancelled. */
  async pickAndUploadFile() {
    const path = await openDialog({ multiple: false, directory: false });
    if (!path) return null;
    return invoke('library_upload_file', { path });
  },
  /** Opens a native save dialog for `filename`; returns true if the file was saved, false if cancelled. */
  async downloadFile(id, filename) {
    const path = await saveDialog({ defaultPath: filename });
    if (!path) return false;
    await invoke('library_download_file', { id, savePath: path });
    return true;
  },
};

/**
 * Chat history — conversations Gaia Cloud has already saved (see
 * conversationStore.js's fire-and-forget save on every turn). Plain JSON
 * over the existing server_request seam; no dedicated Rust command
 * needed, unlike the library's file bytes.
 */
export const historyApi = {
  list: () => serverApi.request(buildHistoryListRequest()).then(parseHistoryList),
  get: (id) => serverApi.request(buildHistoryGetRequest(id)).then(parseHistoryConversation),
  remove: (id) => serverApi.request(buildHistoryDeleteRequest(id)),
};

export const notify = (options) => invoke('notify', { options });
