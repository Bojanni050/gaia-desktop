/**
 * The desktop's single bridge to the Rust shell — and through it, to Gaia Cloud.
 *
 * Every call crosses the ServerLink seam in Rust. The UI never fetches a
 * backend directly and never embeds Gaia Web: this is a client of Gaia
 * Cloud's API, not a browser for another client.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

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

export const notify = (options) => invoke('notify', { options });
