import { describe, it, expect, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

// The hook imports the Rust bridge through server/api.js; nothing here may
// reach a real Tauri command (speech synthesis is the only one a turn can
// trigger, and only for English-looking replies).
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

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
