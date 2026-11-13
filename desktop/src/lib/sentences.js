/**
 * Sentence boundaries for Gaia's voice — used only to decide *how much* of an
 * already-finalized reply is spoken. The desktop keeps whole replies on screen;
 * this module trims only the text handed to speech.
 *
 * Deliberately a small, inspectable splitter rather than an NLP sentence
 * tokenizer, in the same spirit as language.js: Gaia's replies are prose in
 * English or Dutch, and the failure mode that matters is cutting a sentence in
 * half. Terminators are therefore only `.` `!` `?` and `…` followed by
 * whitespace or the end of the text, with guards for the things that merely
 * look like the end of a sentence:
 *
 * - decimals and version numbers (`3.14`, `v2.0`) — no whitespace after the dot
 * - initials, initialisms and abbreviations (`J. Smith`, `U.S.`, `Dr.`, `bijv.`)
 * - numbered list markers at the start of a line (`1. First`)
 *
 * A missed boundary is harmless here (a reply just gets a little more speech
 * than asked for); a wrong one cuts someone off mid-thought. The guards are
 * biased towards missing a boundary.
 */

// Trailing closing punctuation belongs to the sentence it closes: `"Klaar."`,
// `(zie dit.)`, and markdown emphasis/code (`**Klaar.**`).
const SENTENCE_END = /[.!?…]+["'”’»)\]*_`]*(?=\s|$)/gu;

const ABBREVIATIONS = new Set([
  'bijv', 'bijz', 'bv', 'ca', 'dhr', 'dr', 'ds', 'e.g', 'enz', 'etc', 'i.e',
  'jr', 'mevr', 'mr', 'nr', 'prof', 'sr', 'st', 'vs', 'zgn',
]);

// A chain of single letters with dots: `U.S`, `J.R.R`.
const INITIALISM = /^(?:\p{L}\.)+\p{L}?$/u;

/**
 * Whether the terminator starting at `index` really ends a sentence.
 * `text` is the whole string; `index` points at the first `.`/`!`/`?`/`…`.
 */
function endsSentence(text, index) {
  const before = text.slice(0, index);
  const word = before.match(/(\p{L}[\p{L}.]*|\d+)$/u);
  if (!word) return true;
  const token = word[1];
  // A single letter is an initial: "J. Smith", not the end of a sentence.
  if (/^\p{L}$/u.test(token)) return false;
  // A number directly after the start of a line is a list marker ("1. Eerst"),
  // while a year or count inside a sentence ("in 2020.") is a real ending.
  if (/^\d+$/u.test(token)) {
    const lineStart = before.lastIndexOf('\n') + 1;
    return !/^\s*$/u.test(before.slice(lineStart, before.length - token.length));
  }
  return !(ABBREVIATIONS.has(token.toLowerCase()) || INITIALISM.test(token));
}

/**
 * The first `count` sentences of `text`, or `text` unchanged when it has no
 * more than that. Trailing whitespace after the last kept sentence is dropped.
 *
 * @param {string} text
 * @param {number} [count]
 * @returns {string}
 */
export function firstSentences(text, count = 2) {
  const value = String(text ?? '');
  const limit = Number(count);
  if (!Number.isInteger(limit) || limit < 1) return value;

  const pattern = new RegExp(SENTENCE_END.source, 'gu');
  let found = 0;
  let match;
  while ((match = pattern.exec(value)) !== null) {
    if (!endsSentence(value, match.index)) continue;
    found += 1;
    if (found >= limit) return value.slice(0, match.index + match[0].length).trimEnd();
  }
  return value;
}
