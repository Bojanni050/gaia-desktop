/**
 * The wake word as the UI sees it: whether "Hey Gaia" is enrolled and on, how sensitive it is, and the
 * actions on it. It mirrors what Rust reports; the detector, the recordings and the saved features file
 * all live in Rust.
 */
import { useCallback, useEffect, useState } from 'react';
import { voiceApi } from '../server/api';

export function useWakeWord(api = voiceApi) {
  const [status, setStatus] = useState(null);
  const [error, setError] = useState(null);

  const refresh = useCallback(async () => {
    try {
      setStatus(await api.wakeStatus());
    } catch (_) {
      /* the shell is unreachable: keep what is known */
    }
  }, [api]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const run = useCallback(async (call) => {
    setError(null);
    try {
      setStatus(await call());
      return true;
    } catch (e) {
      setError(e && e.code ? { code: e.code, message: e.message } : { code: 'unknown', message: String(e) });
      return false;
    }
  }, []);

  return {
    status,
    error,
    refresh,
    /** Turns "Hey Gaia" on or off (only possible once enrolled). */
    setEnabled: (enabled) => run(() => api.wakeSet(enabled)),
    /** Higher is stricter. Takes effect at once. */
    setSensitivity: (value) => run(() => api.wakeSetSensitivity(value)),
    /** Deletes the enrolled wake word. */
    forget: () => run(() => api.wakeForget()),
    /** Called when enrolment finished with the new status. */
    enrolled: (next) => setStatus(next),
  };
}
