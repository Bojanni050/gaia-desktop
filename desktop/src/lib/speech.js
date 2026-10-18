/**
 * Gaia's voice, from the webview's side: a thin wrapper over the Rust playback engine
 * (`speech_play` / `speech_stop` / `speech_set_gain`).
 *
 * Playback moved into Rust: the audio is fetched, decoded and played there, so the playback signal is
 * available to echo cancellation later and nothing depends on what the webview does when its window is
 * hidden. This module keeps the same shape the rest of the app already uses — one reply speaks at a
 * time, a new reply or turn eases the previous one out — and holds only the local gain so the UI can
 * ask whether playback would be silent.
 */
import { speechApi } from '../server/api';

/**
 * How long an interrupted reply takes to ease out, in ms. Short enough to read as instant, long enough
 * not to click. (Rust's default is the same; sent explicitly so the two cannot drift silently.)
 */
export const SPEECH_FADE_MS = 150;

// Playback gain, applied in Rust. Windows' per-app volume mixer lists the WebView2 host process, not
// "Gaia", so the OS offers no slider for her voice — this gain is the one that actually works. Kept in
// sync with settings.audio by App (on load) and SettingsPanel (on save); defaults to full volume so a
// reply is never silenced by accident.
let gain = { volume: 1, muted: false };
// Whether she speaks her replies at all. Off means no speech is requested (nothing reaches the
// text-to-speech provider); `muted` only silences audio that was still fetched.
let speakReplies = true;

/** Sets the playback gain. `volume` is clamped to 0..1; unknown fields keep their current value. */
export function setSpeechGain(next = {}) {
  if (next.speakReplies !== undefined) {
    speakReplies = !!next.speakReplies;
  }
  if (next.volume !== undefined) {
    gain.volume = Math.min(1, Math.max(0, Number(next.volume)));
  }
  if (next.muted !== undefined) {
    gain.muted = !!next.muted;
  }
  speechApi.setGain(gain.volume, gain.muted).catch(() => {
    /* the shell is not reachable (tests, browser preview): the next reply will use its saved gain */
  });
}

/** Whether she speaks her replies at all (the switch in Settings). */
export function speaksReplies() {
  return speakReplies;
}

/** Whether playback would be silent right now (muted or zero volume). */
export function speechSilenced() {
  return gain.muted || gain.volume <= 0;
}

/**
 * Stops whatever is being spoken (easing it out) and cancels a reply that is still being synthesised.
 * Cutting a clip off on purpose is not a playback failure. No-op when nothing is playing.
 */
export function stopSpeech() {
  speechApi.stop(SPEECH_FADE_MS).catch(() => {});
}

/**
 * Speaks an already-finalized Gaia reply. Synthesis (Gaia Cloud's `/speech/stream`, falling back to
 * `/speech` on an older server) and playback both happen in Rust, and the reply starts playing as
 * soon as the provider has synthesized the first of it; this resolves when the reply ends or is
 * stopped, and rejects if it could not be spoken.
 *
 * @param {string} text
 * @param {'nl'|'en'|null} [language] detected language of the reply; picks the matching voice
 * @returns {Promise<void>}
 */
export async function playSpeech(text, language) {
  const result = await speechApi.play(text, language);
  if (result && result.outcome === 'failed') {
    throw new Error(result.message || 'speech playback failed');
  }
}
