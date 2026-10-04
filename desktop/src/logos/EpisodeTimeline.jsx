/**
 * EpisodeTimeline — the right-hand drawer where Episodes stack.
 *
 * This is the surface the brief asks for: a cognitive timeline, not an activity
 * log. It reads top-to-bottom as interpretation-first cards; the evidence sits
 * collapsed inside each one.
 *
 * The drawer is presentation only. `onAskGaia` / `onSavePattern` are the seams
 * a later step can wire to a conversation or to services/cognition; until then
 * the drawer acknowledges the action so the interaction is honest rather than
 * silent. Escape closes it, matching the other overlays.
 */
import React, { useEffect, useState } from 'react';
import { X } from 'lucide-react';
import EpisodeCard from './EpisodeCard';
import { L } from '../lib/lexicon';

export default function EpisodeTimeline({ episodes = [], onAskGaia, onSavePattern, onClose }) {
  const [notice, setNotice] = useState(null);

  useEffect(() => {
    if (!notice) return undefined;
    const timer = setTimeout(() => setNotice(null), 4200);
    return () => clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if (event.key === 'Escape') onClose && onClose();
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [onClose]);

  const handleAskGaia = (episode) => {
    setNotice(L.logosAskGaiaNotice);
    if (onAskGaia) onAskGaia(episode);
  };

  const handleSavePattern = (episode) => {
    setNotice(L.logosSavePatternNotice);
    if (onSavePattern) onSavePattern(episode);
  };

  return (
    <aside className="logos-drawer" aria-label={L.logosTitle}>
      <header className="logos-drawer-head">
        <div>
          <h2>{L.logosTitle}</h2>
          <p className="logos-drawer-sub">{L.logosHint}</p>
        </div>
        <button className="logos-close" onClick={onClose} aria-label={L.logosClose}>
          <X size={16} />
        </button>
      </header>

      <div className="logos-drawer-body">
        {episodes.length === 0 ? (
          <p className="logos-empty">{L.logosEmpty}</p>
        ) : (
          episodes.map((episode) => (
            <EpisodeCard
              key={episode.id}
              episode={episode}
              onAskGaia={handleAskGaia}
              onSavePattern={handleSavePattern}
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
