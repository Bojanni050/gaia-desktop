import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

// The hook imports the Rust bridge through server/api.js; nothing here may
// reach a real Tauri command (speech synthesis is the only one a turn can
// trigger, and only for English-looking replies).
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
// Playback itself happens in Rust: asserted through, never with. These tests
// verify *whether* Gaia speaks and with which text, not that sound plays.
vi.mock('../lib/speech', () => ({ playSpeech: vi.fn(async () => {}), stopSpeech: vi.fn(), speaksReplies: vi.fn(() => true) }));

import { invoke } from '@tauri-apps/api/core';
import { playSpeech, speaksReplies, stopSpeech } from '../lib/speech';
import { useConversation } from './useConversation';

const tick = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const step = (fields) => ({ id: 'step-1', total: 2, ...fields });

describe('what Gaia keeps in the shared memory while answering', () => {
  const kept = { bank: 'bojan', kind: 'preference', text: 'Bo drinkt graag thee' };

  it('marks the reply when the memory frame comes after the first words', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ content: 'Dat onthoud ik.' });
        onDelta({ memory: kept });
        return 'Dat onthoud ik.';
      }),
    };
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('onthoud dat ik graag thee drink');
    });
    await waitFor(() => expect(result.current.busy).toBe(false));
    const reply = result.current.active.messages.at(-1);
    expect(reply.content).toBe('Dat onthoud ik.');
    expect(reply.kept).toEqual([kept]);
  });

  it('still marks the reply when the memory frame comes before any words', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ memory: kept });
        onDelta({ content: 'Goed, genoteerd.' });
        return 'Goed, genoteerd.';
      }),
    };
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('onthoud dat ik graag thee drink');
    });
    await waitFor(() => expect(result.current.busy).toBe(false));
    const reply = result.current.active.messages.at(-1);
    expect(reply.content).toBe('Goed, genoteerd.');
    expect(reply.kept).toEqual([kept]);
  });

  it('does not mark an ordinary reply, and the memory frame never becomes text', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ content: 'Hallo.' });
        return 'Hallo.';
      }),
    };
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('hoi');
    });
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.active.messages.at(-1).kept).toBeUndefined();
  });
});

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

describe('turn timestamps', () => {
  it('stamps the user turn and the assistant reply with a createdAt', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ content: 'Hallo.' });
        return 'Hallo.';
      }),
    };
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('hoi');
    });

    const messages = result.current.active.messages;
    expect(messages).toHaveLength(2);
    expect(Number.isNaN(Date.parse(messages[0].createdAt))).toBe(false);
    expect(Number.isNaN(Date.parse(messages[1].createdAt))).toBe(false);
  });

  it('stamps a failed reply so the turn still carries a time', async () => {
    const server = {
      streamTurn: vi.fn(async (body, onDelta) => {
        onDelta({ error: 'gaia could not answer right now' });
        return '';
      }),
    };
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('zoek iets wat niet bestaat');
    });

    const last = result.current.active.messages.at(-1);
    expect(last.failed).toBe(true);
    expect(Number.isNaN(Date.parse(last.createdAt))).toBe(false);
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
  });

  it('speaks a Dutch reply when the voice pronounces Dutch', async () => {
    const server = serverWithReply(dutchReply, { configured: true, provider: 'mistral', languages: ['en', 'nl'] });
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(playSpeech).toHaveBeenCalledWith(dutchReply));
    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(result.current.active.messages.at(-1).content).toBe(dutchReply);
  });

  it('makes no speech request at all when speaking replies is switched off', async () => {
    speaksReplies.mockReturnValueOnce(false);
    const server = serverWithReply(dutchReply, { configured: true, provider: 'mistral', languages: ['en', 'nl'] });
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(result.current.busy).toBe(false));
    expect(playSpeech).not.toHaveBeenCalled();
    expect(server.request).not.toHaveBeenCalled(); // not even the voice info
    expect(result.current.active.messages.at(-1).content).toBe(dutchReply);
  });

  it('stays silent on a Dutch reply when the voice is Chinese/English-only', async () => {
    const server = serverWithReply(dutchReply, { configured: true, provider: 'xiaomi', languages: ['zh', 'en'] });
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('kun je dit uitspreken?');
    });

    await waitFor(() => expect(result.current.busy).toBe(false));
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
    expect(playSpeech).not.toHaveBeenCalled();
  });

  it('still speaks an English reply when the server says nothing about its voice', async () => {
    const server = serverWithReply(englishReply, undefined); // legacy behavior preserved
    const { result } = renderHook(() => useConversation(server));
    await act(async () => {
      await result.current.send('can you say this?');
    });

    await waitFor(() => expect(playSpeech).toHaveBeenCalledWith(englishReply));
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

describe('interrupting a turn', () => {
  /** A server whose turn streams one delta and then waits, until it is cancelled (or finished by hand). */
  function hangingServer() {
    const turns = [];
    const server = {
      streamTurn: vi.fn(async (body, onDelta, handle) => {
        const turn = { handle, onDelta };
        turns.push(turn);
        handle.requestId = `req-${turns.length}`;
        onDelta({ content: 'Hoi, ik ben aan het ' });
        return new Promise((resolve, reject) => {
          turn.resolve = resolve;
          turn.reject = reject;
        });
      }),
      cancelTurn: vi.fn(async (requestId) => {
        const turn = turns.find((t) => t.handle.requestId === requestId);
        turn.reject(new Error('turn cancelled'));
        return true;
      }),
    };
    return { server, turns };
  }

  beforeEach(() => {
    playSpeech.mockClear();
    stopSpeech.mockClear();
  });

  it('cancel() closes the turn through the server, keeps the text so far, and shows no error', async () => {
    const { server } = hangingServer();
    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('vertel me iets');
    });
    await waitFor(() => expect(result.current.busy).toBe(true));
    await waitFor(() => expect(result.current.active.messages.at(-1).content).toBe('Hoi, ik ben aan het '));

    let cancelled;
    act(() => {
      cancelled = result.current.cancel();
    });
    expect(cancelled).toBe(true);
    expect(server.cancelTurn).toHaveBeenCalledWith('req-1');
    expect(result.current.busy).toBe(false);
    await act(async () => { await tick(20); });

    const last = result.current.active.messages.at(-1);
    expect(last.content).toBe('Hoi, ik ben aan het ');
    expect(last.failed).toBeUndefined();
    expect(playSpeech).not.toHaveBeenCalled();
    expect(result.current.busy).toBe(false);
  });

  it('cancel() with nothing running does nothing', () => {
    const { server } = hangingServer();
    const { result } = renderHook(() => useConversation(server));
    let cancelled;
    act(() => {
      cancelled = result.current.cancel();
    });
    expect(cancelled).toBe(false);
    expect(server.cancelTurn).not.toHaveBeenCalled();
  });

  it('a late delta from the cancelled turn is dropped', async () => {
    const { server, turns } = hangingServer();
    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('vertel me iets');
    });
    await waitFor(() => expect(turns.length).toBe(1));
    act(() => {
      result.current.cancel();
    });
    act(() => turns[0].onDelta({ content: 'NA DE STOP' }));
    expect(result.current.active.messages.at(-1).content).toBe('Hoi, ik ben aan het ');
  });

  it('a spoken question replaces the running turn, and the old turn finishing late cannot clobber the new one', async () => {
    const { server, turns } = hangingServer();
    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('eerste vraag');
    });
    await waitFor(() => expect(result.current.busy).toBe(true));

    act(() => {
      result.current.send('tweede vraag, hardop', [], { interrupt: true });
    });
    await waitFor(() => expect(turns.length).toBe(2));
    expect(server.cancelTurn).toHaveBeenCalledWith('req-1');
    // let the cancelled first turn's promise settle (its `finally` must not reset the new turn's state)
    await act(async () => { await tick(30); });
    expect(result.current.busy).toBe(true);

    await act(async () => {
      turns[1].resolve('Het antwoord op de tweede vraag.');
      await tick(20);
    });
    await waitFor(() => expect(result.current.busy).toBe(false));
    const userTurns = result.current.active.messages.filter((m) => m.role === 'user').map((m) => m.content);
    expect(userTurns).toEqual(['eerste vraag', 'tweede vraag, hardop']);
  });

  it('without the interrupt option a question during a running turn is still ignored', async () => {
    const { server, turns } = hangingServer();
    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('eerste vraag');
    });
    await waitFor(() => expect(result.current.busy).toBe(true));
    act(() => {
      result.current.send('tweede vraag');
    });
    await act(async () => { await tick(20); });
    expect(turns.length).toBe(1);
    expect(server.cancelTurn).not.toHaveBeenCalled();
  });

  it('hands the server a handle so the turn can be cancelled before any text arrives', async () => {
    let seen;
    const server = {
      streamTurn: vi.fn(async (body, onDelta, handle) => {
        seen = handle;
        handle.requestId = 'early';
        return new Promise(() => {});
      }),
      cancelTurn: vi.fn(async () => true),
    };
    const { result } = renderHook(() => useConversation(server));
    act(() => {
      result.current.send('hallo');
    });
    await waitFor(() => expect(seen).toBeDefined());
    act(() => {
      result.current.cancel();
    });
    expect(server.cancelTurn).toHaveBeenCalledWith('early');
  });
});
