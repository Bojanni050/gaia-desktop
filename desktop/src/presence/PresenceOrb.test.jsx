import React from 'react';
import { describe, it, expect, afterEach } from 'vitest';
import { render, cleanup } from '@testing-library/react';

import { PresenceOrb, STATES } from './PresenceOrb';

afterEach(cleanup);

/** How far one breath swings — the visible size of the pulse. */
const amplitude = (state) => state.scale[1] - state.scale[0];

describe('the presence orb', () => {
  it('breathes in the state it is given', () => {
    const { container } = render(<PresenceOrb state="thinking" size={30} />);
    const wrap = container.querySelector('.presence-orb-wrap');
    expect(wrap.getAttribute('data-state')).toBe('thinking');
    expect(container.querySelectorAll('.presence-orb')).toHaveLength(1);
  });

  it('falls back to quiet for a state it does not know', () => {
    const { container } = render(<PresenceOrb state="nonexistent" />);
    expect(container.querySelector('.presence-orb-wrap').getAttribute('data-state')).toBe('nonexistent');
    // The wrapper still names what it was asked for; the breath it uses is quiet's.
    expect(amplitude(STATES.quiet)).toBeLessThan(amplitude(STATES.thinking));
  });

  it('makes thinking read at a glance — wider, brighter and closer than quiet or listening', () => {
    expect(amplitude(STATES.thinking)).toBeGreaterThan(amplitude(STATES.listening));
    expect(amplitude(STATES.thinking)).toBeGreaterThan(amplitude(STATES.quiet));
    expect(STATES.thinking.coreOpacity[1]).toBeGreaterThan(STATES.quiet.coreOpacity[1]);
    expect(STATES.thinking.coreOpacity[1]).toBeGreaterThan(STATES.listening.coreOpacity[1]);
    expect(STATES.thinking.halo).toBeGreaterThan(STATES.quiet.halo);
    expect(STATES.thinking.breath).toBeLessThan(STATES.quiet.breath);
  });

  it('still reads as composing rather than talking: slower than speech, never over full', () => {
    expect(STATES.thinking.breath).toBeGreaterThan(STATES.speaking.breath);
    expect(STATES.thinking.coreOpacity[1]).toBeLessThanOrEqual(1);
    expect(STATES.quiet.breath).toBeGreaterThan(STATES.listening.breath);
    expect(STATES.listening.breath).toBeGreaterThan(STATES.thinking.breath);
  });
});
