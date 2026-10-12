import { describe, it, expect, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useVoiceStatus } from './useVoiceStatus';

const off = { state: 'off', micEnabled: false, device: null, permission: 'unknown', error: null, glitches: 0 };
const listening = {
  state: 'listening',
  micEnabled: true,
  device: { id: 'a', name: 'Mic A', isDefault: true },
  permission: 'granted',
  error: null,
  glitches: 0,
};

/** A fake voice API whose event handlers the test can fire. */
function fakeApi(overrides = {}) {
  const handlers = {};
  const subscribe = (name) =>
    vi.fn(async (handler) => {
      handlers[name] = handler;
      return () => delete handlers[name];
    });
  const api = {
    getStatus: vi.fn(async () => off),
    listDevices: vi.fn(async () => [{ id: 'a', name: 'Mic A', isDefault: true }]),
    setMicEnabled: vi.fn(async (enabled) => (enabled ? listening : off)),
    selectDevice: vi.fn(async () => listening),
    onState: subscribe('state'),
    onError: subscribe('error'),
    onLevel: subscribe('level'),
    ...overrides,
  };
  return { api, handlers };
}

describe('useVoiceStatus', () => {
  it('starts from what Rust reports: the microphone is off at launch', async () => {
    const { api } = fakeApi();
    const { result } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(result.current.status).toEqual(off));
    expect(api.setMicEnabled).not.toHaveBeenCalled();
    expect(result.current.devices).toHaveLength(1);
  });

  it('the master switch calls Rust and shows the result', async () => {
    const { api } = fakeApi();
    const { result } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    await act(async () => {
      await result.current.setEnabled(true);
    });
    expect(api.setMicEnabled).toHaveBeenCalledWith(true);
    expect(result.current.status.state).toBe('listening');
    await act(async () => {
      await result.current.setEnabled(false);
    });
    expect(api.setMicEnabled).toHaveBeenLastCalledWith(false);
    expect(result.current.status.state).toBe('off');
  });

  it('follows state and level events, and only reports a level while listening', async () => {
    const { api, handlers } = fakeApi();
    const { result } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(handlers.state).toBeTypeOf('function'));
    act(() => handlers.level(0.4));
    expect(result.current.level).toBe(0); // off: no level shown
    act(() => handlers.state(listening));
    act(() => handlers.level(0.4));
    expect(result.current.level).toBe(0.4);
    act(() => handlers.state({ ...off, state: 'error', error: { code: 'no_frames', message: 'x' } }));
    expect(result.current.level).toBe(0);
  });

  it('keeps the latest notice and clears it when the microphone is switched on again', async () => {
    const { api, handlers } = fakeApi();
    const { result } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(handlers.error).toBeTypeOf('function'));
    act(() => handlers.error({ code: 'device_fallback', message: 'gone' }));
    expect(result.current.notice.code).toBe('device_fallback');
    await act(async () => {
      await result.current.setEnabled(true);
    });
    expect(result.current.notice).toBeNull();
  });

  it('selects a device by id, or the system default with null', async () => {
    const { api } = fakeApi();
    const { result } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    await act(async () => {
      await result.current.selectDevice('a');
    });
    expect(api.selectDevice).toHaveBeenLastCalledWith('a');
    await act(async () => {
      await result.current.selectDevice('');
    });
    expect(api.selectDevice).toHaveBeenLastCalledWith(null);
  });

  it('survives an unreachable shell and unsubscribes on unmount', async () => {
    const { api, handlers } = fakeApi({
      getStatus: vi.fn(async () => {
        throw new Error('no shell');
      }),
      setMicEnabled: vi.fn(async () => {
        throw new Error('no shell');
      }),
    });
    const { result, unmount } = renderHook(() => useVoiceStatus(api));
    await waitFor(() => expect(handlers.state).toBeTypeOf('function'));
    await act(async () => {
      await result.current.setEnabled(true);
    });
    expect(result.current.status).toBeNull();
    unmount();
    await waitFor(() => expect(handlers.state).toBeUndefined());
  });
});
