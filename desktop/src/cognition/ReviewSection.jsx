/**
 * Cognition review — the human Absolute Override, inline in the sidebar like
 * History. These are the derived statements Logos is still weighing
 * (hypotheses, candidate models, open questions, relationships); the person
 * confirms one or lets it go. Only this verdict reaches `confirmed` — no
 * model, no accumulation of evidence on its own. The list loads lazily on
 * first expand, never at launch.
 *
 * Each row renders as an EpistemicReviewCard: evidence and the quarantined
 * counter-hypothesis can be opened, and a macro statement additionally demands
 * a conscious implication choice before Confirm unlocks. Every call goes over
 * the generic server_request seam and is wrapped as { status, body }.
 */
import React, { useEffect, useState } from 'react';
import { ChevronRight, Lightbulb } from 'lucide-react';
import { cognitionApi } from '../server/api';
import { L } from '../lib/lexicon';
import EpistemicReviewCard from './EpistemicReviewCard';

export default function ReviewSection() {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState(null); // null = not loaded yet
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);

  const load = () =>
    cognitionApi
      .list()
      .then(setItems)
      .catch(() => {
        setItems([]);
        setError(L.cognitionLoadFailed);
      });

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next && items === null) load();
  };

  const afterAction = (id, updated) => {
    setItems((prev) => (prev || []).map((it) => (it.id === id ? { ...it, ...(updated || {}) } : it)));
  };

  const run = async (item, action, apply) => {
    if (busyId) return;
    setBusyId(item.id);
    setError(null);
    try {
      const response = await action();
      apply(response);
    } catch (_) {
      setError(L.cognitionActionFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleConfirm = (item, { rationale, statement } = {}) =>
    run(item, () => cognitionApi.confirm(item.id, { rationale, statement }),
      (response) => afterAction(item.id, { ...((response && response.body) || {}), status: 'confirmed' }));

  const handleReject = (item) =>
    run(item, () => cognitionApi.reject(item.id, L.cognitionRejected),
      () => afterAction(item.id, { status: 'rejected' }));

  const handleTest = (item) =>
    run(item, () => cognitionApi.test(item.id),
      (response) => afterAction(item.id, { ...((response && response.body) || {}), status: 'testing' }));

  return (
    <div className="sidebar-section">
      <button className="sidebar-section-header" onClick={toggle} aria-expanded={open}>
        <ChevronRight size={13} className={`sidebar-section-chevron${open ? ' open' : ''}`} />
        <Lightbulb size={13} />
        <span>{L.cognition}</span>
      </button>

      {open && (
        <div className="sidebar-section-body">
          {error && <div className="sidebar-section-error">{error}</div>}
          {items === null ? (
            <p className="sidebar-section-hint">{L.cognitionLoading}</p>
          ) : items.length === 0 ? (
            <p className="sidebar-section-hint">{L.cognitionEmpty}</p>
          ) : (
            <>
              <p className="sidebar-section-hint">{L.cognitionHint}</p>
              {items.map((item) => (
                <EpistemicReviewCard
                  key={item.id}
                  item={item}
                  busy={busyId === item.id}
                  onConfirm={handleConfirm}
                  onReject={handleReject}
                  onTest={handleTest}
                />
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
