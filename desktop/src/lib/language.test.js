import { describe, it, expect } from 'vitest';
import { looksEnglish, shouldSpeak } from './language';

describe('looksEnglish', () => {
  it('recognizes an English sentence', () => {
    expect(looksEnglish('Yes. It feels good to be here, and that is not nothing.')).toBe(true);
  });

  it('recognizes a Dutch sentence as not English', () => {
    expect(looksEnglish('Ja. Het voelt goed om er te zijn, en dat is niet niks.')).toBe(false);
  });

  it('defaults to false (silence) for empty or signal-free text', () => {
    expect(looksEnglish('')).toBe(false);
    expect(looksEnglish('   ')).toBe(false);
    expect(looksEnglish(undefined)).toBe(false);
    expect(looksEnglish('Xiaomi MiMo 2026')).toBe(false); // no function-word signal either way
  });

  it('defaults to false on a tie', () => {
    // "de" (NL) vs "the" (EN) — contrived, but a tie must not default to
    // speaking a possibly-Dutch reply.
    expect(looksEnglish('de the')).toBe(false);
  });

  it('is not fooled by a single shared/borrowed word', () => {
    expect(looksEnglish('Gaia')).toBe(false);
  });
});

describe('shouldSpeak', () => {
  const DUTCH = 'Ja. Het voelt goed om er te zijn, en dat is niet niks.';
  const ENGLISH = 'Yes. It feels good to be here, and that is not nothing.';

  it('speaks any non-empty reply when the voice pronounces Dutch', () => {
    expect(shouldSpeak(DUTCH, ['en', 'nl'])).toBe(true);
    expect(shouldSpeak(ENGLISH, ['en', 'nl'])).toBe(true);
  });

  it('keeps the English-only gate for a Chinese/English-only voice', () => {
    expect(shouldSpeak(ENGLISH, ['zh', 'en'])).toBe(true);
    expect(shouldSpeak(DUTCH, ['zh', 'en'])).toBe(false);
  });

  it('falls back to the English-only gate when the voice is unknown', () => {
    expect(shouldSpeak(ENGLISH, undefined)).toBe(true);
    expect(shouldSpeak(DUTCH, undefined)).toBe(false);
    expect(shouldSpeak(ENGLISH, [])).toBe(true);
    expect(shouldSpeak(DUTCH, [])).toBe(false);
  });

  it('never speaks empty text, whatever the voice', () => {
    expect(shouldSpeak('', ['en', 'nl'])).toBe(false);
    expect(shouldSpeak('   ', ['en', 'nl'])).toBe(false);
    expect(shouldSpeak(undefined, ['en', 'nl'])).toBe(false);
  });
});
