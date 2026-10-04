/**
 * EpisodeTimeline — the right-hand drawer where Episodes stack.
 *
 * This is the surface the brief asks for: a cognitive timeline, not an activity
 * log. It reads top-to-bottom as interpretation-first cards; the evidence sits
 * collapsed inside each one.
 *
 * It loads real episodes from Gaia Cloud over the `episodeApi` seam (GET
 * /kairos/episodes, the Kairos synthesis pipeline). The drawer's three honest
 * states matter: loading, empty, and *failed*. A failed fetch is shown as a
 * failure, never silently swapped for fake episodes.
 *
 * `episodes` may be passed in as a prop (the demo/mock path); when it is, no
 * fetch runs. Otherwise the drawer fetches on open.
 */
import React, { useEffect, useMemo, useState } from 'react';
import { X } from 'lucide-react';
import EpisodeCard from './EpisodeCard';
import { episodeApi, serverApi } from '../server/api';
import { adaptKairosEpisode } from '../state/contract';
import { useKairosLiveEpisodes } from '../state/useKairosLiveEpisodes';
import { L } from '../lib/lexicon';

export default function EpisodeTimeline({ episodes, onClose }) {
  const injected = Array.isArray(episodes);
  const [items, setItems] = useState(episodes || null);
  const [error, setError] = useState(null);
  const [notice, setNotice] = useState(null);
  const { episodes: liveEpisodes, status: liveStatus } = useKairosLiveEpisodes(serverApi);

  useEffect(() => {
    if (injected) return undefined;
    let alive = true;
    episodeApi
      .list()
      .then((list) => { if (alive) setItems(list); })
      .catch(() => { if (alive) { setItems([]); setError(L.logosLoadFailed); } });
    return () => { alive = false; };
  }, [injected]);
  // Merge the fetched history with live arrivals, newest first and de-duplicated
  // by id (a live episode may also already be in the fetched page).
  const merged = useMemo(() => {
    if (injected) return items || [];
    const base = items || [];
    const seen = new Set(base.map((e) => e.id));
    const fresh = liveEpisodes
      .map(adaptKairosEpisode)
      .filter((e) => e && !seen.has(e.id));
    return [...fresh, ...base];
  }, [items, liveEpisodes, injected]);

  useEffect(() => {
    if (!notice) return undefined;
    const timer = setTimeout(() => setNotice(null), 4200);
    return () => clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.key === 'Escape' && onClose) onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const handleAskGaia = (episode) => {
    setNotice(L.logosAskGaiaNotice);
  };

  return (
    <aside className="logos-drawer" aria-label={L.logosTitle}>
      <header className="logos-drawer-head">
        <div>
          <h2>
            {L.logosTitle}
            {!injected && (
              <span
                className={`logos-live-dot ${liveStatus}`}
                aria-label={liveStatus === 'connected' ? L.logosLive : liveStatus === 'offline' ? L.logosOffline : L.logosConnecting}
                title={liveStatus === 'connected' ? L.logosLive : liveStatus === 'offline' ? L.logosOffline : L.logosConnecting}
              />
            )}
          </h2>
          <p className="logos-drawer-sub">{L.logosHint}</p>
        </div>
        <button className="logos-close" onClick={onClose} aria-label={L.logosClose}>
          <X size={16} />
        </button>
      </header>
      <div className="logos-drawer-body">
        {error && <div className="logos-error">{error}</div>}
        {items === null ? (
          <p className="logos-empty">{L.logosLoading}</p>
        ) : merged.length === 0 ? (
          <p className="logos-empty">{L.logosEmpty}</p>
        ) : (
          merged.map((episode) => (
            <EpisodeCard
              key={episode.id}
              episode={episode}
              onAskGaia={handleAskGaia}
              loadEvidence={injected ? undefined : episodeApi.evidence}
            />
          ))
        )}
      </div>

      {notice && (
        <div className="logos-notice" role="status">
          {notice}
        </div>
      )}
    </aside>
  );
}
