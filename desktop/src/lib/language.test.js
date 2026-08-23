import { describe, it, expect } from 'vitest';
import { looksEnglish } from './language';

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
