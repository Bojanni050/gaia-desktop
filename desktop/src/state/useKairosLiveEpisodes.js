/**
 * Live Kairos episodes, pushed from Gaia Cloud over the `server://episode`
 * Tauri event (Rust relays gaia-api's `kairos/episodes/stream` SSE). Returns
 * the episodes synthesised since this hook mounted, newest first, plus a live
 * connection state for the status indicator.
 *
 * Deliberately additive and non-authoritative: the timeline still loads its
 * history over the REST seam (GET /kairos/episodes). This hook only supplies
 * what arrives *live*, so a dropped connection degrades to "no live arrivals"
 * rather than a broken list. The caller merges the two and de-duplicates by id.
 */
import { useEffect, useState } from 'react';

export function useKairosLiveEpisodes(server) {
  const [episodes, setEpisodes] = useState([]);
  const [status, setStatus] = useState('connecting');

  useEffect(() => {
    let unlisten;
    let active = true;

    server
      .onEpisode((episode) => {
        if (!active || !episode || !episode.id) return;
        setStatus('connected');
        setEpisodes((prev) => {
          if (prev.some((e) => e.id === episode.id)) return prev;
          return [episode, ...prev];
        });
      })
      .then((unlistenFn) => {
        unlisten = unlistenFn;
      })
      .catch(() => {
        if (active) setStatus('offline');
      });

    return () => {
      active = false;
      if (unlisten) unlisten();
    };
  }, [server]);

  return { episodes, status };
}
