import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

// The hook has its own tests; here it is a stub the test controls, so the Composer's own rules
// (what happens to a transcript) are what is under test.
let ptt;
let captured;
vi.mock('../state/usePushToTalk', () => ({
  usePushToTalk: ({ onTranscript }) => {
    captured = onTranscript;
    return ptt;
  },
}));

import Composer from './Composer';

const hear = (text) => act(() => captured(text));

afterEach(cleanup);

beforeEach(() => {
  ptt = { phase: 'idle', error: null, available: true, micOn: true, toggle: vi.fn(), cancel: vi.fn() };
  captured = undefined;
});

describe('Composer: spoken text', () => {
  it('sends what was said as an ordinary turn when nothing is running and nothing is typed', () => {
    const onSend = vi.fn();
    render(<Composer onSend={onSend} busy={false} />);
    hear('  wat is het weer vandaag  ');
    expect(onSend).toHaveBeenCalledWith('wat is het weer vandaag', []);
  });

  it('puts it in the text box instead of sending when a turn is already running', () => {
    const onSend = vi.fn();
    render(<Composer onSend={onSend} busy />);
    hear('en nog iets');
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox').value).toBe('en nog iets');
  });

  it('appends to a half-written message and never sends over it', () => {
    const onSend = vi.fn();
    render(<Composer onSend={onSend} busy={false} />);
    fireEvent.change(screen.getByRole('textbox'), { target: { value: 'Ik wil weten' } });
    hear('hoe laat het is');
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox').value).toBe('Ik wil weten hoe laat het is');
  });

  it('ignores an empty transcript', () => {
    const onSend = vi.fn();
    render(<Composer onSend={onSend} busy={false} />);
    hear('   ');
    expect(onSend).not.toHaveBeenCalled();
    expect(screen.getByRole('textbox').value).toBe('');
  });
});

describe('Composer: the mic button', () => {
  it('is hidden when Gaia Cloud cannot transcribe', () => {
    ptt.available = false;
    render(<Composer onSend={vi.fn()} busy={false} />);
    expect(screen.queryByRole('button', { name: /spreken|talk|microfoon aanzetten|switch the microphone on/i })).toBeNull();
  });

  it('a click is handed to the push-to-talk hook', () => {
    render(<Composer onSend={vi.fn()} busy={false} />);
    fireEvent.click(document.querySelector('.composer-mic'));
    expect(ptt.toggle).toHaveBeenCalledTimes(1);
  });

  it('shows recording, microphone-off and transcribing states', () => {
    ptt.phase = 'recording';
    const { rerender } = render(<Composer onSend={vi.fn()} busy={false} />);
    const button = document.querySelector('.composer-mic');
    expect(button.className).toContain('is-recording');
    expect(button.getAttribute('aria-pressed')).toBe('true');

    ptt = { ...ptt, phase: 'idle', micOn: false };
    rerender(<Composer onSend={vi.fn()} busy={false} />);
    expect(document.querySelector('.composer-mic').className).toContain('is-off');

    ptt = { ...ptt, phase: 'transcribing', micOn: true };
    rerender(<Composer onSend={vi.fn()} busy={false} />);
    expect(document.querySelector('.composer-mic').disabled).toBe(true);
  });

  it('explains "no speech" and failures in a calm sentence', () => {
    ptt.phase = 'noSpeech';
    const { rerender } = render(<Composer onSend={vi.fn()} busy={false} />);
    expect(screen.getByRole('status').textContent).toMatch(/niets gehoord|did not hear/i);

    ptt = { ...ptt, phase: 'failed', error: { code: 'unreachable', message: 'x' } };
    rerender(<Composer onSend={vi.fn()} busy={false} />);
    expect(screen.getByRole('status').textContent).toMatch(/bereiken|reach/i);

    ptt = { ...ptt, phase: 'failed', error: { code: 'something_new', message: 'x' } };
    rerender(<Composer onSend={vi.fn()} busy={false} />);
    expect(screen.getByRole('status').textContent.length).toBeGreaterThan(10);
  });
});
