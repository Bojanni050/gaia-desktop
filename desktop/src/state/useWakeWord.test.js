import { describe, it, expect, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { useWakeWord } from './useWakeWord';

const status = (over = {}) => ({
  enrolled: true,
  enabled: false,
  sensitivity: 0.55,
  minSensitivity: 0.4,
  maxSensitivity: 0.7,
  clips: 0,
  minClips: 5,
  maxClips: 8,
  ...over,
});

describe('useWakeWord', () => {
  it('loads the status from Rust', async () => {
    const api = { wakeStatus: vi.fn(async () => status()) };
    const { result } = renderHook(() => useWakeWord(api));
    await waitFor(() => expect(result.current.status).toEqual(status()));
  });

  it('turns the wake word on and mirrors what Rust reports', async () => {
    const api = {
      wakeStatus: vi.fn(async () => status()),
      wakeSet: vi.fn(async (enabled) => status({ enabled })),
    };
    const { result } = renderHook(() => useWakeWord(api));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    let ok;
    await act(async () => {
      ok = await result.current.setEnabled(true);
    });
    expect(ok).toBe(true);
    expect(api.wakeSet).toHaveBeenCalledWith(true);
    expect(result.current.status.enabled).toBe(true);
  });

  it('keeps the last status and reports the error when Rust refuses', async () => {
    const api = {
      wakeStatus: vi.fn(async () => status({ enrolled: false })),
      wakeSet: vi.fn(async () => {
        throw { code: 'not_enrolled', message: 'no wake word yet' };
      }),
    };
    const { result } = renderHook(() => useWakeWord(api));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    let ok;
    await act(async () => {
      ok = await result.current.setEnabled(true);
    });
    expect(ok).toBe(false);
    expect(result.current.error.code).toBe('not_enrolled');
    expect(result.current.status.enrolled).toBe(false);
  });

  it('forgets the wake word and applies a new sensitivity', async () => {
    const api = {
      wakeStatus: vi.fn(async () => status()),
      wakeSetSensitivity: vi.fn(async (v) => status({ sensitivity: v })),
      wakeForget: vi.fn(async () => status({ enrolled: false, enabled: false })),
    };
    const { result } = renderHook(() => useWakeWord(api));
    await waitFor(() => expect(result.current.status).not.toBeNull());
    await act(async () => {
      await result.current.setSensitivity(0.6);
    });
    expect(result.current.status.sensitivity).toBe(0.6);
    await act(async () => {
      await result.current.forget();
    });
    expect(result.current.status.enrolled).toBe(false);
  });
});
