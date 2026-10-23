/**
 * The microphone as the UI sees it: Rust's `MicController` status, kept fresh through the `voice://`
 * events, plus the two actions the user has (the master switch and the input device).
 *
 * The master switch is runtime-only on the Rust side: the microphone is off at every launch, so this
 * hook never "restores" a previous on-state. It owns no audio logic: it mirrors what Rust reports.
 */
import { useCallback, useEffect, useState } from 'react';
import { voiceApi } from '../server/api';

export function useVoiceStatus(api = voiceApi) {
  const [status, setStatus] = useState(null);
  const [level, setLevel] = useState(0);
  const [gate, setGate] = useState(null);
  const [devices, setDevices] = useState([]);
  // The latest failure or notice from `voice://error` (cleared when the mic is switched on again).
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    let active = true;
    const unlisten = [];
    const keep = (promise) =>
      Promise.resolve(promise)
        .then((fn) => {
          if (!active) fn && fn();
          else if (fn) unlisten.push(fn);
        })
        .catch(() => {});

    api.getStatus().then((s) => active && setStatus(s)).catch(() => {});
    api.listDevices().then((d) => active && setDevices(d || [])).catch(() => {});
    keep(api.onState((s) => active && setStatus(s)));
    keep(api.onLevel((l) => active && setLevel(Number(l) || 0)));
    keep(api.onGate((g) => active && setGate(g || null)));
    keep(api.onError((e) => active && setNotice(e)));
    return () => {
      active = false;
      unlisten.forEach((fn) => fn());
    };
  }, [api]);

  const setEnabled = useCallback(
    async (enabled) => {
      if (enabled) setNotice(null);
      try {
        setStatus(await api.setMicEnabled(enabled));
      } catch (_) {
        /* the shell is unreachable: leave the last known status in place */
      }
      if (!enabled) setLevel(0);
    },
    [api]
  );

  const selectDevice = useCallback(
    async (id) => {
      try {
        setStatus(await api.selectDevice(id || null));
      } catch (_) {
        /* settings could not be written: keep the old selection */
      }
    },
    [api]
  );

  const refreshDevices = useCallback(
    () => api.listDevices().then((d) => setDevices(d || [])).catch(() => {}),
    [api]
  );

  return {
    status,
    level: status && status.state === 'listening' ? level : 0,
    // The gate's view of the input only while the microphone is listening; no stream, no level to show.
    gate: status && status.state === 'listening' ? gate : null,
    devices,
    notice,
    setEnabled,
    selectDevice,
    refreshDevices,
  };
}
