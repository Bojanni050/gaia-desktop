import { describe, it, expect, beforeEach } from 'vitest';

// Node has no localStorage; the lexicon proxy needs one. Minimal stub.
const store = {};
globalThis.localStorage = {
  getItem: (k) => store[k] ?? null,
  setItem: (k, v) => { store[k] = String(v); },
  removeItem: (k) => { delete store[k]; },
};

import { phraseTurnError } from './phrases';
import { LANGUAGES } from '../lib/lexicon';

// The lexicon proxy reads localStorage; ensure a defined language per test.
const setLang = (lang) => localStorage.setItem('gaia.lang', lang);

describe('phraseTurnError', () => {
  beforeEach(() => setLang('nl'));

  it('phrases an unconfigured server calmly, pointing at settings', () => {
    const phrase = phraseTurnError({ kind: 'communication', message: 'communication error: no Gaia Server configured' });
    expect(phrase).toBe(LANGUAGES.nl.turnNoServer);
    expect(phrase).toMatch(/instellingen/i);
    expect(phrase).not.toMatch(/error|failed|40\d/i);
  });

  it('phrases an unreachable server without transport details', () => {
    const phrase = phraseTurnError({ kind: 'communication', message: 'communication error: request failed: connection refused' });
    expect(phrase).toBe(LANGUAGES.nl.turnUnreachable);
    expect(phrase).not.toMatch(/fetch|http|connection refused/i);
  });

  it('never leaks raw messages for unknown kinds', () => {
    const phrase = phraseTurnError(new Error('stack trace detail'));
    expect(phrase).toBe(LANGUAGES.nl.turnFallback);
    expect(phrase).not.toMatch(/stack trace/);
  });

  it('follows the chosen language', () => {
    setLang('en');
    const phrase = phraseTurnError({ kind: 'communication', message: 'communication error: no Gaia Server configured' });
    expect(phrase).toBe(LANGUAGES.en.turnNoServer);
    setLang('nl');
  });
});
