import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

// The upload itself is api.js's own concern (see api.test.js); here it is a
// stub, so what paste does with the clipboard is what is under test. The rest
// of the module stays real — VoiceControls runs off voiceApi from it.
const uploadPastedFile = vi.fn();
vi.mock('../server/api', async (importOriginal) => {
  const actual = await importOriginal();
  return {
    ...actual,
    libraryApi: { ...actual.libraryApi, uploadPastedFile: (...args) => uploadPastedFile(...args) },
  };
});

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
  uploadPastedFile.mockReset();
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

describe('Composer: a pasted file', () => {
  // jsdom's File has no arrayBuffer(); every webview that runs this does.
  const file = (name, type, bytes = [137, 80, 78, 71]) => {
    const pasted = new File([new Uint8Array(bytes)], name, { type });
    Object.defineProperty(pasted, 'arrayBuffer', {
      value: async () => new Uint8Array(bytes).buffer,
    });
    return pasted;
  };

  const paste = (pasted, extraItems = []) =>
    act(async () => {
      fireEvent.paste(screen.getByRole('textbox'), {
        clipboardData: {
          items: [
            ...(pasted ? [{ kind: 'file', type: pasted.type, getAsFile: () => pasted }] : []),
            ...extraItems,
          ],
          files: pasted ? [pasted] : [],
        },
      });
    });

  it('uploads a nameless clipboard image and shows it as a chip, not as text', async () => {
    uploadPastedFile.mockResolvedValue({ id: 'f1', filename: 'pasted-image-x.png' });
    const onSend = vi.fn();
    render(<Composer onSend={onSend} busy={false} />);

    await paste(file('image.png', 'image/png'));

    expect(uploadPastedFile).toHaveBeenCalledTimes(1);
    const [bytes, name] = uploadPastedFile.mock.calls[0];
    expect(bytes).toBeInstanceOf(Uint8Array);
    expect(bytes.length).toBe(4);
    expect(name).toMatch(/^pasted-image-\d{8}-\d{6}\.png$/);
    expect(screen.getByText('pasted-image-x.png')).toBeTruthy();
    expect(screen.getByRole('textbox').value).toBe('');
    expect(onSend).not.toHaveBeenCalled();
  });

  it('keeps the name a file copied in Explorer already had', async () => {
    uploadPastedFile.mockResolvedValue({ id: 'f2', filename: 'rapport.pdf' });
    render(<Composer onSend={vi.fn()} busy={false} />);

    await paste(file('rapport.pdf', 'application/pdf'));

    expect(uploadPastedFile.mock.calls[0][1]).toBe('rapport.pdf');
  });

  it('leaves pasted text alone', async () => {
    render(<Composer onSend={vi.fn()} busy={false} />);

    await paste(null, [{ kind: 'string', type: 'text/plain' }]);

    expect(uploadPastedFile).not.toHaveBeenCalled();
  });

  it('says calmly that a pasted file did not make it', async () => {
    uploadPastedFile.mockRejectedValue(new Error('unreachable'));
    render(<Composer onSend={vi.fn()} busy={false} />);

    await paste(file('image.png', 'image/png'));

    expect(screen.getByRole('status').textContent).toMatch(/niet gelukt|could not/i);
    expect(screen.queryByText(/pasted-image/)).toBeNull();
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
