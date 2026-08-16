import { describe, it, expect } from 'vitest';
import { phraseTurnError } from './phrases';

describe('phraseTurnError', () => {
  it('phrases an unconfigured server calmly', () => {
    const phrase = phraseTurnError({ kind: 'communication', message: 'communication error: no Gaia Server configured' });
    expect(phrase).toMatch(/settings/i);
    expect(phrase).not.toMatch(/error|failed|40\d/i);
  });

  it('phrases an unreachable server without transport details', () => {
    const phrase = phraseTurnError({ kind: 'communication', message: 'communication error: request failed: connection refused' });
    expect(phrase).not.toMatch(/fetch|http|connection refused/i);
  });

  it('never leaks raw messages for unknown kinds', () => {
    const phrase = phraseTurnError(new Error('stack trace detail'));
    expect(phrase).not.toMatch(/stack trace/);
  });
});
