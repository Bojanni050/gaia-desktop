import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

vi.mock('../lib/speech', () => ({ stopSpeech: vi.fn(), playSpeech: vi.fn(), setSpeechGain: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { stopSpeech } from '../lib/speech';
import { useVoiceSession } from './useVoiceSession';

function fakeApi({ listening = false } = {}) {
  const handlers = {};
  const sub = (name) =>
    vi.fn(async (handler) => {
      handlers[name] = handler;
      return () => delete handlers[name];
    });
  return {
    api: {
      continuousState: vi.fn(async () => listening),
      onContinuous: sub('continuous'),
      onTranscript: sub('transcript'),
      onBargeIn: sub('barge'),
    },
    handlers,
  };
}

const ready = (handlers) =>
  waitFor(() => {
    expect(handlers.continuous).toBeTypeOf('function');
    expect(handlers.transcript).toBeTypeOf('function');
    expect(handlers.barge).toBeTypeOf('function');
  });

beforeEach(() => stopSpeech.mockClear());

describe('useVoiceSession', () => {
  it('starts off, and follows hands-free state events', async () => {
    const { api, handlers } = fakeApi();
    const conversation = { send: vi.fn(), cancel: vi.fn() };
    const { result } = renderHook(() => useVoiceSession({ conversation, api }));
    await ready(handlers);
    expect(result.current.state).toBe('off');
    expect(result.current.active).toBe(false);

    act(() => handlers.continuous({ state: 'listening' }));
    expect(result.current.state).toBe('listening');
    expect(result.current.active).toBe(true);
    act(() => handlers.continuous({ state: 'hearing' }));
    expect(result.current.state).toBe('hearing');
    act(() => handlers.continuous({ state: 'failed', code: 'unreachable', message: 'x' }));
    expect(result.current.error.code).toBe('unreachable');
    act(() => handlers.continuous({ state: 'off' }));
    expect(result.current.active).toBe(false);
    expect(result.current.error).toBeNull();
  });

  it('picks up hands-free listening that was already running', async () => {
    const { api, handlers } = fakeApi({ listening: true });
    const { result } = renderHook(() => useVoiceSession({ conversation: { send: vi.fn(), cancel: vi.fn() }, api }));
    await ready(handlers);
    await waitFor(() => expect(result.current.state).toBe('listening'));
  });

  it('sends a hands-free transcript as an ordinary turn that replaces one still running', async () => {
    const { api, handlers } = fakeApi();
    const conversation = { send: vi.fn(), cancel: vi.fn() };
    renderHook(() => useVoiceSession({ conversation, api }));
    await ready(handlers);
    act(() => handlers.transcript({ text: '  wat is het weer  ', language: 'nl', source: 'continuous' }));
    expect(conversation.send).toHaveBeenCalledWith('wat is het weer', [], { interrupt: true });
  });

  it('leaves push-to-talk transcripts to the composer', async () => {
    const { api, handlers } = fakeApi();
    const conversation = { send: vi.fn(), cancel: vi.fn() };
    renderHook(() => useVoiceSession({ conversation, api }));
    await ready(handlers);
    act(() => handlers.transcript({ text: 'dit is van de knop', source: 'ptt' }));
    act(() => handlers.transcript({ text: 'geen bron' }));
    expect(conversation.send).not.toHaveBeenCalled();
  });

  it('ignores an empty transcript', async () => {
    const { api, handlers } = fakeApi();
    const conversation = { send: vi.fn(), cancel: vi.fn() };
    renderHook(() => useVoiceSession({ conversation, api }));
    await ready(handlers);
    act(() => handlers.transcript({ text: '   ', source: 'continuous' }));
    expect(conversation.send).not.toHaveBeenCalled();
  });

  it('a barge-in stops her voice and cancels the turn still streaming', async () => {
    const { api, handlers } = fakeApi();
    const conversation = { send: vi.fn(), cancel: vi.fn() };
    renderHook(() => useVoiceSession({ conversation, api }));
    await ready(handlers);
    act(() => handlers.barge());
    expect(stopSpeech).toHaveBeenCalledTimes(1);
    expect(conversation.cancel).toHaveBeenCalledTimes(1);
  });

  it('always talks to the latest conversation, not the one it first saw', async () => {
    const { api, handlers } = fakeApi();
    const first = { send: vi.fn(), cancel: vi.fn() };
    const second = { send: vi.fn(), cancel: vi.fn() };
    const { rerender } = renderHook(({ c }) => useVoiceSession({ conversation: c, api }), { initialProps: { c: first } });
    await ready(handlers);
    rerender({ c: second });
    act(() => handlers.transcript({ text: 'hallo', source: 'continuous' }));
    expect(first.send).not.toHaveBeenCalled();
    expect(second.send).toHaveBeenCalledTimes(1);
  });

  it('unsubscribes on unmount', async () => {
    const { api, handlers } = fakeApi();
    const { unmount } = renderHook(() => useVoiceSession({ conversation: { send: vi.fn(), cancel: vi.fn() }, api }));
    await ready(handlers);
    unmount();
    await waitFor(() => expect(handlers.continuous).toBeUndefined());
    expect(handlers.transcript).toBeUndefined();
    expect(handlers.barge).toBeUndefined();
  });
});
