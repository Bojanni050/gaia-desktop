/**
 * Plays back audio bytes already synthesized server-side (serverApi's
 * speechApi.synthesize → Gaia Cloud's `/speech` → src/speech/mimoTts.js).
 *
 * Deliberately not a second audio engine: the webview already has one
 * (the native `Audio` element), so this is just the thinnest possible
 * wrapper around it — turn bytes into a Blob, play it, release the object
 * URL when done. No mixing, no queuing, no device selection; those are
 * exactly the kind of concerns a "build it later if actually needed"
 * engine would own, and V1 doesn't need one.
 *
 * Only one reply is ever spoken at a time: a new reply (or a new turn)
 * supersedes the last, so the previous playback is stopped rather than
 * left to overlap — Gaia doesn't talk over herself.
 */

/**
 * @param {Uint8Array} bytes
 * @param {string} [mimeType]
 * @returns {Promise<void>} resolves once playback finishes (or rejects if it fails to start)
 */

// Playback gain, applied locally. Windows' per-app volume mixer lists the
// WebView2 host process, not "Gaia", so the OS offers no slider for her
// voice — this module-level gain is the one that actually works. Kept in
// sync with settings.audio by App (on load) and SettingsPanel (on save);
// defaults to full volume so a reply is never silenced by accident.
let gain = { volume: 1, muted: false };

/** Sets the playback gain. `volume` is clamped to 0..1; unknown fields keep their current value. */
export function setSpeechGain(next = {}) {
  if (next.volume !== undefined) {
    gain.volume = Math.min(1, Math.max(0, Number(next.volume)));
  }
  if (next.muted !== undefined) {
    gain.muted = !!next.muted;
  }
}

/** Whether playback would be silent right now (muted or zero volume). */
export function speechSilenced() {
  return gain.muted || gain.volume <= 0;
}

// The one clip currently speaking, if any. Held so a new reply can stop it
// before starting — see stopSpeech.
let current = null;

/**
 * Stops whatever is currently being spoken, if anything. Resolves the
 * pending playSpeech: cutting a clip off on purpose (a new reply, or a new
 * turn) is not a playback failure. No-op when nothing is playing.
 */
export function stopSpeech() {
  if (!current) return;
  const { audio, url, resolve } = current;
  current = null;
  try {
    audio.pause();
  } catch (_) {
    /* already gone — nothing to stop */
  }
  URL.revokeObjectURL(url);
  resolve();
}

/**
 * @param {Uint8Array} bytes
 * @param {string} [mimeType]
 * @returns {Promise<void>} resolves once playback finishes (or is stopped, or rejects if it fails to start)
 */
export function playSpeech(bytes, mimeType = 'audio/wav') {
  // Barge-in: a new reply always supersedes the previous one.
  stopSpeech();

  const blob = new Blob([bytes], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const audio = new Audio(url);
  audio.volume = speechSilenced() ? 0 : gain.volume;

  return new Promise((resolve, reject) => {
    const settle = (fn, value) => {
      if (current?.audio === audio) current = null;
      URL.revokeObjectURL(url);
      fn(value);
    };
    audio.addEventListener('ended', () => settle(resolve));
    audio.addEventListener('error', () => settle(reject, new Error('audio playback failed')));
    current = { audio, url, resolve };
    audio.play().catch((error) => settle(reject, error));
  });
}
