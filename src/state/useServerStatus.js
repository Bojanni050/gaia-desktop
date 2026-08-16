/**
 * Connection status of the Gaia Cloud link, as maintained by the Rust
 * ServerLink. Pure reachability — never a claim about Gaia's inner state.
 */
import { useEffect, useState } from 'react';

export function useServerStatus(server) {
  const [status, setStatus] = useState('connecting');

  useEffect(() => {
    let unlisten;
    let active = true;

    server
      .getStatus()
      .then((payload) => {
        if (active) setStatus(payload?.status || 'notConfigured');
      })
      .catch(() => {
        if (active) setStatus('notConfigured');
      });

    server
      .onStatus((payload) => setStatus(typeof payload === 'string' ? payload : payload?.status))
      .then((unlistenFn) => {
        unlisten = unlistenFn;
      })
      .catch(() => {});

    return () => {
      active = false;
      if (unlisten) unlisten();
    };
  }, [server]);

  return status;
}
