import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

// The hook imports the Rust bridge through server/api.js; nothing here may
// reach a real Tauri command (speech synthesis is the only one a turn can
// trigger, and only for English-looking replies).
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
// Playback itself is a thin wrapper over the webview's Audio element —
// asserted through, never with: these tests verify *whether* Gaia speaks
// and with which bytes/mime, not that jsdom can play sound.
vi.mock('../lib/speech', () => ({ playSpeech: vi.fn(async () => {}), stopSpeech: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import { playSpeech, stopSpeech } from '../lib/speech';
import { useConversation } from './useConversation';

const tick = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const step = (fields) => ({ id: 'step-1', total: 2, ...fields });

describe('plan progress during a streamed turn', () => {
  it('shows which step is running, then clears it as soon as the answer streams', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ step: step({ index: 1, type: 'retrieval', status: 'start' }) });
        await tick(30); // the plan is still working — this window is what progress exists for
        onDelta({ step: step({ index: 1, type: 'retrieval', status: 'done' }) });
        onDelta({ step: step({ index: 2, type: 'generation', status: 'start' }) });
        onDelta({ content: 'Hier is het volledige antwoord.' });
        return 'Hier is het volledige antwoord.';
      }),
    };

    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('wat zei ik vorige maand over emigreren?');
    });

    await waitFor(() =>
      expect(result.current.progress).toMatchObject({
        index: 1,
        total: 2,
        type: 'retrieval',
        status: 'start',
      })
    );

    // The frame is progress, never content: it must not have become a
    // message of its own, and it must be gone by the time the reply lands.
    await waitFor(() => expect(result.current.progress).toBeNull());
    await waitFor(() => expect(result.current.busy).toBe(false));
    const messages = result.current.active.messages;
    expect(messages.filter((m) => m.role === 'assistant')).toHaveLength(1);
    expect(messages.at(-1).content).toBe('Hier is het volledige antwoord.');
  });

  it('keeps the generic thinking state when the server sends no progress', async () => {
    const server = {
      // A single-action turn: content deltas, no step frames at all.
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ content: 'Ja, dat klopt.' });
        return 'Ja, dat klopt.';
      }),
    };

    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('klopt het dat jij alles onthoudt?');
    });

    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.progress).toBeNull();
    expect(result.current.active.messages.at(-1).content).toBe('Ja, dat klopt.');
  });

  it('reports a failure the server stated on the open stream, never a blank bubble', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ step: step({ index: 1, type: 'retrieval', status: 'start' }) });
        onDelta({ error: 'gaia could not answer right now' });
        return ''; // headers were open (progress went out), so failure arrives on the stream
      }),
    };

    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('zoek iets wat niet bestaat');
    });

    const message = result.current.active.messages.at(-1);
    expect(message.failed).toBe(true);
    expect(message.content.length).toBeGreaterThan(0); // a calm phrase, not an empty bubble
    expect(result.current.progress).toBeNull();
    expect(result.current.busy).toBe(false);
  });
});

describe("Gaia's voice gate", () => {
  const dutchReply = 'Ja. Het voelt goed om er te zijn, en dat is niet niks.';
  const englishReply = 'Yes. It feels good to be here, and that is not nothing.';

  const serverWithReply = (reply, info) => ({
    streamTurn: vi.fn(async (body, onDelta) => {
      onDelta({ content: reply });
      return reply;
    }),
    // Absent entirely for legacy servers — the hook must not require it.
    ...(info === undefined ? {} : { request: vi.fn(async () => ({ body: info })) }),
  });

  beforeEach(() => {
    invoke.mockClear();
    playSpeech.mockClear();
    stopSpeech.mockClear();
    invoke.mockResolvedValue({ audio: [1, 2, 3], mime_type: 'audio/mpeg' });
  });

  it('speaks a Dutch reply when the voice pronounces Dutch', async () => {
    const server = serverWithReply(dutchReply, { configured: true, provider: 'mistral', languages: ['en', 'nl'] });
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('speech_synthesize', { text: dutchReply }));
    await waitFor(() =>
      expect(playSpeech).toHaveBeenCalledWith(new Uint8Array([1, 2, 3]), 'audio/mpeg')
    );
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.active.messages.at(-1).content).toBe(dutchReply);
  });

  it('stays silent on a Dutch reply when the voice is Chinese/English-only', async () => {
    const server = serverWithReply(dutchReply, { configured: true, provider: 'xiaomi', languages: ['zh', 'en'] });
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(invoke).not.toHaveBeenCalledWith('speech_synthesize', expect.anything());
    expect(playSpeech).not.toHaveBeenCalled();
    expect(result.current.active.messages.at(-1).content).toBe(dutchReply);
  });

  it('stays silent on a Dutch reply when the server says nothing about its voice', async () => {
    const server = serverWithReply(dutchReply, undefined); // no request function at all
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(invoke).not.toHaveBeenCalledWith('speech_synthesize', expect.anything());
    expect(playSpeech).not.toHaveBeenCalled();
  });

  it('still speaks an English reply when the server says nothing about its voice', async () => {
    const server = serverWithReply(englishReply, undefined); // legacy behavior preserved
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('can you say this?');
    });

    await waitFor(() => expect(invoke).toHaveBeenCalledWith('speech_synthesize', { text: englishReply }));
  });

  it('cuts the previous reply off the moment a new turn begins', async () => {
    const server = serverWithReply(englishReply, undefined);
    const { result } = renderHook(() => useConversation(server));

    await act(async () => {
      await result.current.send('say this once');
    });
    stopSpeech.mockClear(); // the first turn's own stop is not what this asserts

    await act(async () => {
      await result.current.send('now say this instead');
    });

    expect(stopSpeech).toHaveBeenCalledTimes(1);
  });
});
