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
 * supersedes the last, so the previous playback is faded out and stopped
 * rather than left to overlap — Gaia doesn't talk over herself, and a cut
 * reply eases out instead of clicking off mid-syllable.
 */

/**
 * How long an interrupted reply takes to ease out, in ms. Short enough to
 * read as instant, long enough not to click.
 */
export const SPEECH_FADE_MS = 150;

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
 * Stops whatever is currently being spoken, easing it out first. Resolves
 * the pending playSpeech: cutting a clip off on purpose (a new reply, or a
 * new turn) is not a playback failure. No-op when nothing is playing.
 */
export function stopSpeech() {
  if (!current) return;
  const clip = current;
  current = null;
  fadeOut(clip);
}

/** Ramps a clip's volume to zero, then pauses it and releases its URL. */
function fadeOut(clip) {
  const from = clip.audio.volume;
  const began = Date.now();
  clip.timer = setInterval(() => {
    const progress = Math.min(1, (Date.now() - began) / SPEECH_FADE_MS);
    try {
      clip.audio.volume = from * (1 - progress);
    } catch (_) {
      /* element already gone — nothing left to silence */
    }
    if (progress >= 1) finish(clip);
  }, 16);
}

/** Ends a clip for good: stop any fade, pause, release the blob, resolve. */
function finish(clip) {
  if (clip.timer) {
    clearInterval(clip.timer);
    clip.timer = null;
  }
  try {
    clip.audio.pause();
  } catch (_) {
    /* already gone */
  }
  URL.revokeObjectURL(clip.url);
  clip.resolve();
}

/** A normal end (or a failure) — same cleanup, but the promise settles as before. */
function settle(clip, fn, value) {
  if (current === clip) current = null;
  if (clip.timer) {
    clearInterval(clip.timer);
    clip.timer = null;
  }
  URL.revokeObjectURL(clip.url);
  fn(value);
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
    const clip = { audio, url, resolve, timer: null };
    current = clip;
    audio.addEventListener('ended', () => settle(clip, resolve));
    audio.addEventListener('error', () => settle(clip, reject, new Error('audio playback failed')));
    audio.play().catch((error) => settle(clip, reject, error));
  });
}
