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
  buildCognitionListRequest,
  buildCognitionTestRequest,
  buildCognitionRejectRequest,
  buildCognitionReopenRequest,
  buildCognitionConfirmRequest,
  parseCognitionList,
  buildEpisodeListRequest,
  buildEpisodeEvidenceRequest,
  parseEpisodeList,
  parseEpisodeEvidence,
} from '../state/contract';

let streamRequestCounter = 0;

export const serverApi = {
  getConfig: () => invoke('server_get_config'),
  applyConfig: (config) => invoke('server_set_config', { config }),
  getStatus: () => invoke('server_get_status'),
  testConnection: () => invoke('server_test_connection'),
  request: (request) => invoke('server_request', { request }),
  getCloudVersion: () => invoke('server_get_cloud_version'),
  getDesktopVersion: () => invoke('version_get_desktop'),
  onStatus: (handler) => listen('server://status', (event) => handler(event.payload)),
  /**
   * Realtime events relayed from Gaia Cloud (ServerLink::spawn_event_bridge,
   * backed by gaia-api's `conversations/events` SSE endpoint) — today just
   * `{ topic: 'conversation.history.changed', payload: null }` whenever any
   * client saves or deletes a conversation, so History can refresh without
   * polling. `handler` receives the raw `ServerEvent` envelope.
   */
  onServerEvent: (handler) => listen('server://event', (event) => handler(event.payload)),
  /**
   * Live Kairos episodes relayed from Gaia Cloud (ServerLink::spawn_episode_bridge,
   * backed by gaia-api's `kairos/episodes/stream` SSE endpoint). `handler`
   * receives one raw episode object per synthesis. Returns an unlisten fn.
   */
  onEpisode: (handler) => listen('server://episode', (event) => handler(event.payload)),
  /**
   * Streams one turn (Rust's `server_stream_turn`, over gaia-api's SSE
   * path — turn.js's performStreamingTurn). `onDelta` is called with
   * `{ content, reasoningContent }` as each piece arrives, and optionally
   * with the server's two extension fields instead of content:
   * `{ step }` — plan progress (`{ id, index, total, type, status }`,
   * which step of a multi-step plan is running) and `{ error }` — the
   * server's calm failure wording on an already-open stream. Neither
   * carries content; resolves with the full assistant text once the stream
   * ends, rejects the same way `request` does on any transport/server
   * failure.
   */
  streamTurn: async (body, onDelta) => {
    const requestId = `turn-${Date.now()}-${streamRequestCounter++}`;
    const unlisten = await listen('server://turn-delta', (event) => {
      if (event.payload?.requestId !== requestId) return;
      onDelta(event.payload);
    });
    try {
      return await invoke('server_stream_turn', { body, requestId });
    } finally {
      unlisten();
    }
  },
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

export const mcpApi = {
  listTools: (serverId) => invoke('mcp_list_tools', { serverId }),
  callTool: (serverId, tool, arguments_) =>
    invoke('mcp_call_tool', { serverId, tool, arguments: arguments_ }),
};

export const captureApi = {
  listSources: () => invoke('capture_list_sources'),
};

export const audioApi = {
  getStatus: () => invoke('audio_get_status'),
};

/**
 * Gaia's voice — synthesizes speech for an already-received Gaia reply via
 * Rust's `speech_synthesize` (gaia-api's `POST /speech`, src/speech/
 * mimoTts.js or mistralTts.js server-side, depending on the configured TTS
 * provider). `text` must be a finished Gaia response, never a fresh
 * prompt: this module has no say in what Gaia says, only how it sounds.
 * Rust returns `{ audio, mime_type }` (Tauri serializes the bytes as a
 * plain JS array of byte values); `synthesize` turns that into
 * `{ bytes, mimeType }` the caller can hand to playSpeech for Blob
 * playback. Older shells returned the bare byte array — still accepted,
 * labeled `audio/wav` as before.
 */
export const speechApi = {
  async synthesize(text) {
    const result = await invoke('speech_synthesize', { text });
    if (Array.isArray(result)) {
      return { bytes: new Uint8Array(result), mimeType: 'audio/wav' };
    }
    return {
      bytes: new Uint8Array(result.audio),
      mimeType: result.mime_type || 'audio/wav',
    };
  },
};

/**
 * Describes the server's configured voice (gaia-api's `GET /speech/info`):
 * `{ configured, provider, languages }`. The desktop gates *whether* to
 * speak on `languages` (see lib/language.js's shouldSpeak) — read from
 * the provider, never judged from the reply text.
 *
 * `request` is serverApi.request (or any compatible `{ method, path }`
 * function). Never throws: an unreachable server, an older server without
 * this endpoint, or a missing request function all resolve to
 * `undefined`, and callers fall back to the legacy English-only gate —
 * staying quiet stays the safe failure mode.
 */
export async function getSpeechInfo(request) {
  if (typeof request !== 'function') return undefined;
  try {
    const response = await request({ method: 'get', path: 'speech/info' });
    return response?.body;
  } catch (_) {
    return undefined;
  }
}

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
 * conversationStore.js's fire-and-forget save on every turn). List, read
 * and delete are plain JSON over the existing server_request seam. Export
 * is a file download, not opaque JSON — the server sends markdown as
 * plain text — so it has its own Rust command that streams the bytes to
 * a local path chosen through a native save dialog, same as the library.
 */
export const historyApi = {
  list: () => serverApi.request(buildHistoryListRequest()).then(parseHistoryList),
  get: (id) => serverApi.request(buildHistoryGetRequest(id)).then(parseHistoryConversation),
  remove: (id) => serverApi.request(buildHistoryDeleteRequest(id)),
  /** Opens a native save dialog for the export filename; returns true if
   * the file was written, false if the dialog was cancelled. */
  async export(id, format, filename) {
    const path = await saveDialog({ defaultPath: filename });
    if (!path) return false;
    await invoke('history_export_conversation', { id, format, savePath: path });
    return true;
  },
};

export const notify = (options) => invoke('notify', { options });

/**
 * Cognition review — the derived statements Logos is still weighing, and the
 * human verdicts on them (`/cognition/*` on Gaia Cloud). `confirm` is the only
 * path to `confirmed`; it may name older statements to supersede. Every call
 * is plain JSON over the generic server_request seam.
 */
export const cognitionApi = {
  list: () => serverApi.request(buildCognitionListRequest()).then(parseCognitionList),
  test: (id) => serverApi.request(buildCognitionTestRequest(id)),
  reject: (id, reason) => serverApi.request(buildCognitionRejectRequest(id, reason)),
  reopen: (id, reason) => serverApi.request(buildCognitionReopenRequest(id, reason)),
  confirm: (id, options) => serverApi.request(buildCognitionConfirmRequest(id, options)),
};

/**
 * Kairos episodes — the time blocks Kairos recognised, each an interpretation
 * plus the captures behind it. `list` reads them; `evidence` loads the raw
 * observations for one episode on demand (the audit path). Plain JSON over the
 * generic server_request seam. Endpoint is outside this repo: a server that
 * predates it answers an error, which the drawer surfaces rather than hides.
 */
export const episodeApi = {
  list: (query) => serverApi.request(buildEpisodeListRequest(query)).then(parseEpisodeList),
  evidence: (id) => serverApi.request(buildEpisodeEvidenceRequest(id)).then(parseEpisodeEvidence),
};
