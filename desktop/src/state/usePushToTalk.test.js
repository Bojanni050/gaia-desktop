import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { usePushToTalk, NOTICE_MS } from './usePushToTalk';

const off = { state: 'off', micEnabled: false, device: null, permission: 'unknown', error: null, glitches: 0 };
const on = { ...off, state: 'listening', micEnabled: true, permission: 'granted' };

/** A fake voice API; the test fires its events by hand. */
function fakeApi({ status = off, pttStart } = {}) {
  const handlers = {};
  const sub = (name) =>
    vi.fn(async (handler) => {
      handlers[name] = handler;
      return () => delete handlers[name];
    });
  const api = {
    getStatus: vi.fn(async () => status),
    listDevices: vi.fn(async () => []),
    setMicEnabled: vi.fn(async (enabled) => (enabled ? on : off)),
    selectDevice: vi.fn(async () => on),
    onState: sub('state'),
    onError: sub('error'),
    onLevel: sub('level'),
    pttStart: pttStart || vi.fn(async () => {}),
    pttStop: vi.fn(async () => {}),
    pttCancel: vi.fn(async () => {}),
    onPtt: sub('ptt'),
    onTranscript: sub('transcript'),
  };
  return { api, handlers };
}

const sttInfo = (info) => vi.fn(async () => ({ body: info }));

async function ready(handlers) {
  await waitFor(() => expect(handlers.ptt).toBeTypeOf('function'));
  await waitFor(() => expect(handlers.transcript).toBeTypeOf('function'));
}

describe('usePushToTalk', () => {
  it('with the microphone off, the button switches the microphone on and does not record', async () => {
    const { api, handlers } = fakeApi();
    const { result } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
    await ready(handlers);
    await waitFor(() => expect(result.current.micOn).toBe(false));
    await act(async () => {
      await result.current.toggle();
    });
    expect(api.setMicEnabled).toHaveBeenCalledWith(true);
    expect(api.pttStart).not.toHaveBeenCalled();
  });

  it('with the microphone on, one click starts a recording and the next stops it', async () => {
    const { api, handlers } = fakeApi({ status: on });
    const { result } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
    await ready(handlers);
    await waitFor(() => expect(result.current.micOn).toBe(true));

    await act(async () => {
      await result.current.toggle();
    });
    expect(api.pttStart).toHaveBeenCalledTimes(1);
    act(() => handlers.ptt({ state: 'recording' }));
    expect(result.current.phase).toBe('recording');

    await act(async () => {
      await result.current.toggle();
    });
    expect(api.pttStop).toHaveBeenCalledTimes(1);
    act(() => handlers.ptt({ state: 'transcribing' }));
    expect(result.current.phase).toBe('transcribing');

    // a click while transcribing is ignored
    await act(async () => {
      await result.current.toggle();
    });
    expect(api.pttStart).toHaveBeenCalledTimes(1);
    expect(api.pttStop).toHaveBeenCalledTimes(1);
  });

  it('hands the transcript text to the caller, untouched', async () => {
    const { api, handlers } = fakeApi({ status: on });
    const onTranscript = vi.fn();
    renderHook(() => usePushToTalk({ api, onTranscript, request: sttInfo({ configured: true }) }));
    await ready(handlers);
    act(() => handlers.transcript({ text: 'wat is het weer', language: 'nl' }));
    expect(onTranscript).toHaveBeenCalledWith('wat is het weer', { text: 'wat is het weer', language: 'nl' });
  });

  it('a failed start (the microphone dropped out) shows the error and settles back to idle', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const pttStart = vi.fn(async () => {
        throw { code: 'mic_off', message: 'switch the microphone on first' };
      });
      const { api, handlers } = fakeApi({ status: on, pttStart });
      const { result } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
      await ready(handlers);
      await waitFor(() => expect(result.current.micOn).toBe(true));
      await act(async () => {
        await result.current.toggle();
      });
      expect(result.current.phase).toBe('failed');
      expect(result.current.error.code).toBe('mic_off');
      await act(async () => {
        await vi.advanceTimersByTimeAsync(NOTICE_MS + 50);
      });
      expect(result.current.phase).toBe('idle');
    } finally {
      vi.useRealTimers();
    }
  });

  it('reports the failure code Rust sends, and "no speech" settles back to idle', async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const { api, handlers } = fakeApi({ status: on });
      const { result } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
      await ready(handlers);
      act(() => handlers.ptt({ state: 'failed', code: 'unreachable', message: 'x' }));
      expect(result.current.error.code).toBe('unreachable');
      act(() => handlers.ptt({ state: 'noSpeech' }));
      expect(result.current.phase).toBe('noSpeech');
      expect(result.current.error).toBeNull();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(NOTICE_MS + 50);
      });
      expect(result.current.phase).toBe('idle');
    } finally {
      vi.useRealTimers();
    }
  });

  it('Escape cancels a recording, and only while recording', async () => {
    const { api, handlers } = fakeApi({ status: on });
    const { result } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
    await ready(handlers);
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(api.pttCancel).not.toHaveBeenCalled();

    act(() => handlers.ptt({ state: 'recording' }));
    expect(result.current.phase).toBe('recording');
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(api.pttCancel).toHaveBeenCalledTimes(1);
  });

  it('is unavailable only when Gaia Cloud says it cannot transcribe', async () => {
    const a = fakeApi({ status: on });
    const unavailable = renderHook(() => usePushToTalk({ api: a.api, request: sttInfo({ configured: false }) }));
    await waitFor(() => expect(unavailable.result.current.available).toBe(false));

    const b = fakeApi({ status: on });
    const unknown = renderHook(() => usePushToTalk({ api: b.api, request: vi.fn(async () => { throw new Error('offline'); }) }));
    await ready(b.handlers);
    expect(unknown.result.current.available).toBe(true);

    const c = fakeApi({ status: on });
    const noRequest = renderHook(() => usePushToTalk({ api: c.api, request: undefined }));
    await ready(c.handlers);
    expect(noRequest.result.current.available).toBe(true);
  });

  it('unsubscribes from the events on unmount', async () => {
    const { api, handlers } = fakeApi({ status: on });
    const { unmount } = renderHook(() => usePushToTalk({ api, request: sttInfo({ configured: true }) }));
    await ready(handlers);
    unmount();
    await waitFor(() => expect(handlers.ptt).toBeUndefined());
    expect(handlers.transcript).toBeUndefined();
  });
});

beforeEach(() => vi.clearAllMocks());
afterEach(() => vi.useRealTimers());
