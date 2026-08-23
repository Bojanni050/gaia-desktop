/**
 * Lightweight EN/NL language heuristic — used only to decide whether
 * Gaia's spoken voice (speechApi.synthesize, via Xiaomi's
 * mimo-v2.5-tts-voicedesign) should be invoked for a given reply.
 *
 * mimo-v2.5-tts-voicedesign only supports Chinese and English voice
 * timbres/pronunciation (confirmed against Xiaomi's docs, 2026-08) — Dutch
 * text gets mispronounced rather than rejected, so this must be checked
 * *before* calling speechApi at all, not left to the TTS call to fail
 * loudly. This is a presentation-layer gate on *whether to speak*, not a
 * cognitive judgment about what was said — the same posture the rest of
 * this hook already keeps toward TTS (see useConversation.js's own
 * comment).
 *
 * Deliberately a small, inspectable stopword-count heuristic — the same
 * idiom gaia-api's own intentIQ.js/foundation.js already use for
 * bilingual EN/NL signal detection — not a real language-ID model. A
 * reply with no confident signal either way defaults to silence: staying
 * quiet is the safe failure mode here, not guessing and mispronouncing.
 */

function boundary(word) {
  return new RegExp(`\\b${word}\\b`, 'i');
}

// Function words are a much stronger signal than content words (which are
// often shared, borrowed, or proper nouns in both languages) and don't
// require stripping punctuation/casing beyond what \b already handles.
const ENGLISH_SIGNALS = [
  'the', 'and', 'is', 'are', 'you', 'your', 'with', 'that', 'this', 'have',
  'not', 'what', 'when', 'how', 'because', 'about', 'would', 'could', 'been',
].map(boundary);

const DUTCH_SIGNALS = [
  'de', 'het', 'een', 'en', 'is', 'zijn', 'je', 'jij', 'met', 'dat', 'dit',
  'niet', 'wat', 'wanneer', 'hoe', 'omdat', 'over', 'zou', 'kon', 'geweest',
].map(boundary);

function scoreText(text, patterns) {
  return patterns.reduce((n, p) => n + (p.test(text) ? 1 : 0), 0);
}

/**
 * @param {string} text
 * @returns {boolean} true only when English signal strictly outweighs
 *   Dutch signal; false for Dutch, ties, and no-signal (empty/very short/
 *   neither-language) text alike.
 */
export function looksEnglish(text) {
  const value = String(text || '');
  const englishScore = scoreText(value, ENGLISH_SIGNALS);
  const dutchScore = scoreText(value, DUTCH_SIGNALS);
  return englishScore > dutchScore;
}
