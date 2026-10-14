import { describe, it, expect } from 'vitest';
import { LANGUAGES } from './lexicon';

// A key present in one language only renders as its raw name in the other
// (the L proxy falls back to the key itself), so the two must stay in step.
describe('lexicon', () => {
  it('has the same keys in every language', () => {
    const nl = Object.keys(LANGUAGES.nl);
    const en = Object.keys(LANGUAGES.en);
    expect(nl.filter((k) => !en.includes(k))).toEqual([]);
    expect(en.filter((k) => !nl.includes(k))).toEqual([]);
  });
});
