import { describe, it, expect } from 'vitest';
import { stepProgressLabel } from './stepProgress';

/**
 * The server's `step` frame is a fact (position, step type, status); this
 * module is the presentation half — how that fact reads to a person while
 * a multi-step plan is still running.
 */
describe('stepProgressLabel', () => {
  it('renders each plan step type in Gaia\'s own activity words', () => {
    expect(stepProgressLabel({ index: 1, total: 2, type: 'retrieval' })).toBe(
      'Step 1 of 2 - searching...'
    );
    expect(stepProgressLabel({ index: 1, total: 2, type: 'reasoning' })).toBe(
      'Step 1 of 2 - thinking it through...'
    );
    expect(stepProgressLabel({ index: 2, total: 2, type: 'generation' })).toBe(
      'Step 2 of 2 - answering...'
    );
    expect(stepProgressLabel({ index: 1, total: 1, type: 'capability' })).toBe(
      'Step 1 of 1 - looking it up...'
    );
  });

  it('falls back to a neutral activity for a step type it does not know', () => {
    expect(stepProgressLabel({ index: 3, total: 4, type: 'someday-thing' })).toBe(
      'Step 3 of 4 - working...'
    );
  });

  it('never names machinery — activity, not the capability behind it', () => {
    const label = stepProgressLabel({
      id: 'step-1',
      index: 1,
      total: 3,
      type: 'retrieval',
      status: 'start',
      capability: 'conversation_search',
    });
    expect(label).not.toContain('conversation_search');
    expect(label).not.toContain('step-1');
  });

  it('returns null when there is no progress to show', () => {
    expect(stepProgressLabel(null)).toBeNull();
    expect(stepProgressLabel(undefined)).toBeNull();
    expect(stepProgressLabel({})).toBeNull();
    expect(stepProgressLabel({ type: 'retrieval' })).toBeNull();
  });
});
