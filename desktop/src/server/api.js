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
  /**
   * Cancels a streaming turn that is in flight (the user interrupted): Rust closes the connection, so
   * Gaia Cloud sees the disconnect and can stop. The pending `streamTurn` rejects; the text already
   * received stays. Resolves to false when that turn had already finished.
   */
  cancelTurn: (requestId) => invoke('server_cancel_turn', { requestId }),
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
  streamTurn: async (body, onDelta, handle) => {
    const requestId = `turn-${Date.now()}-${streamRequestCounter++}`;
    // Handed to the caller before anything is sent, so the turn can be cancelled at any moment.
    if (handle) handle.requestId = requestId;
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
 * The microphone: Rust's `MicController`. The master switch is runtime-only: the microphone is off at
 * every launch and only an explicit `setMicEnabled(true)` opens it; `false` drops the capture stream so
 * Windows releases the device. Status is `{ state: 'off'|'listening'|'error', micEnabled, device,
 * permission, error: { code, message } | null, glitches }`. Nothing is recorded or sent anywhere yet.
 */
export const voiceApi = {
  getStatus: () => invoke('voice_get_status'),
  listDevices: () => invoke('voice_list_devices'),
  setMicEnabled: (enabled) => invoke('voice_set_mic_enabled', { enabled }),
  /** `id` is a device id from listDevices, or null for the system default. Persisted. */
  selectDevice: (id) => invoke('voice_select_device', { id }),
  onState: (handler) => listen('voice://state', (event) => handler(event.payload)),
  /** Failures and notices: `{ code, message }`. */
  onError: (handler) => listen('voice://error', (event) => handler(event.payload)),
  /** Peak input level 0..1, a few times a second while listening. */
  onLevel: (handler) => listen('voice://level', (event) => handler(event.payload)),

  // Push-to-talk: Rust records one utterance (in memory only), trims it to the speech, has Gaia Cloud
  // transcribe it, and reports through events. Silence is never uploaded. `pttStart` rejects with
  // `{ code, message }` (`mic_off`: switch the microphone on first; `busy`).
  pttStart: () => invoke('voice_ptt_start'),
  pttStop: () => invoke('voice_ptt_stop'),
  pttCancel: () => invoke('voice_ptt_cancel'),
  pttState: () => invoke('voice_ptt_state'),
  /** `{ state: 'idle'|'recording'|'transcribing'|'noSpeech'|'failed', code?, message? }` */
  onPtt: (handler) => listen('voice://ptt', (event) => handler(event.payload)),
  /** `{ text, language, source }`: what was said, as Gaia Cloud heard it. `source` is `'ptt'` (the mic
   * button) or `'continuous'` (hands-free); each owner acts only on its own. Send it as an ordinary turn. */
  onTranscript: (handler) => listen('voice://transcript', (event) => handler(event.payload)),

  // Hands-free listening: the microphone stays open and speech is found as it happens. The preference
  // is saved; it starts when the microphone is on (switching the microphone on is always the user's act).
  continuousSet: (enabled) => invoke('voice_continuous_set', { enabled }),
  continuousState: () => invoke('voice_continuous_state'),
  /** Whether talking over Gaia interrupts her (otherwise her speech is simply not listened to). */
  setBargeIn: (enabled) => invoke('voice_set_barge_in', { enabled }),
  /** `{ state: 'off'|'listening'|'hearing'|'transcribing'|'failed', code?, message? }` */
  onContinuous: (handler) => listen('voice://continuous', (event) => handler(event.payload)),
  /** The user started talking over Gaia. Rust has already stopped her voice; cancel the turn too. */
  onBargeIn: (handler) => listen('voice://barge-in', () => handler()),

  // The wake word, "Hey Gaia": recognised on this device only; nothing is transcribed or sent until it is
  // heard. Enrolled by the user (a few recordings, kept in memory only; what is saved is a features file, not
  // audio) and calibrated to their voice.
  /** `{ enrolled, enabled, sensitivity, minSensitivity, maxSensitivity, clips, minClips, maxClips }` */
  wakeStatus: () => invoke('voice_wake_status'),
  /** Records one enrolment clip (about 3 s). `{ check: { ok, problem, seconds, peak }, accepted, min, max }` */
  wakeEnrollRecord: () => invoke('voice_wake_enroll_record'),
  /** Builds and calibrates the wake word from the accepted clips; resolves with the new status. */
  wakeEnrollFinish: () => invoke('voice_wake_enroll_finish'),
  wakeEnrollCancel: () => invoke('voice_wake_enroll_cancel'),
  wakeSet: (enabled) => invoke('voice_wake_set', { enabled }),
  wakeSetSensitivity: (value) => invoke('voice_wake_set_sensitivity', { value }),
  /** Deletes the enrolled wake word. */
  wakeForget: () => invoke('voice_wake_forget'),
  /** "Hey Gaia" was heard (the chime plays). */
  onWake: (handler) => listen('voice://wake', () => handler()),
};

/**
 * Describes Gaia Cloud's speech recognition (`GET /speech/stt-info`): `{ configured, provider,
 * languages, maxBytes }`. Never throws: an unreachable or older server resolves to `undefined`, and the
 * caller assumes it might work (a real failure explains itself when it happens).
 */
export async function getSttInfo(request) {
  if (typeof request !== 'function') return undefined;
  try {
    const response = await request({ method: 'get', path: 'speech/stt-info' });
    return response?.body;
  } catch (_) {
    return undefined;
  }
}

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
  /**
   * Synthesizes `text` (Gaia Cloud's `/speech`) and plays it in Rust. Resolves with
   * `{ outcome: 'ended' | 'stopped' }`, or `{ outcome: 'failed', message }`; rejects if the
   * synthesis request itself fails.
   */
  play: (text) => invoke('speech_play', { text }),
  /** Eases the current reply out over `fadeMs` and cancels one still being synthesised. */
  stop: (fadeMs) => invoke('speech_stop', { fadeMs }),
  setGain: (volume, muted) => invoke('speech_set_gain', { volume, muted }),
  /** Output devices Gaia's voice can play through: `[{ id, name, isDefault }]`. */
  listOutputDevices: () => invoke('speech_list_output_devices'),
  /** `id` is a device id from listOutputDevices, or null for the system default. Persisted. */
  selectOutputDevice: (id) => invoke('speech_select_output_device', { id }),
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
