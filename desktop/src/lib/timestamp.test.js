import { describe, it, expect } from 'vitest';
import { formatTurnTime } from './timestamp';

describe('formatTurnTime', () => {
  it('formats an ISO timestamp as wall-clock date and time', () => {
    // Constructed locally (not a Z string) so the assertion holds in any TZ.
    const local = new Date(2026, 9, 3, 14, 32, 7).toISOString();
    expect(formatTurnTime(local)).toBe('03-10-2026 14:32');
  });

  it('pads single-digit days, months, hours and minutes', () => {
    const local = new Date(2026, 0, 5, 9, 4, 0).toISOString();
    expect(formatTurnTime(local)).toBe('05-01-2026 09:04');
  });

  it('returns null for a missing or unparseable value, never "Invalid Date"', () => {
    expect(formatTurnTime(undefined)).toBeNull();
    expect(formatTurnTime('')).toBeNull();
    expect(formatTurnTime('not a date')).toBeNull();
  });
});
