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

export function playSpeech(bytes, mimeType = 'audio/wav') {
  const blob = new Blob([bytes], { type: mimeType });
  const url = URL.createObjectURL(blob);
  const audio = new Audio(url);
  audio.volume = speechSilenced() ? 0 : gain.volume;

  const cleanup = () => URL.revokeObjectURL(url);

  return new Promise((resolve, reject) => {
    audio.addEventListener('ended', () => {
      cleanup();
      resolve();
    });
    audio.addEventListener('error', () => {
      cleanup();
      reject(new Error('audio playback failed'));
    });
    audio.play().catch((error) => {
      cleanup();
      reject(error);
    });
  });
}
