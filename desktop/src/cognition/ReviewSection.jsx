/**
 * Cognition review — the human Absolute Override, inline in the sidebar like
 * History. These are the derived statements Logos is still weighing
 * (hypotheses, candidate models, open questions, relationships); the person
 * confirms one or lets it go. Only this verdict reaches `confirmed` — no
 * model, no accumulation of evidence on its own. The list loads lazily on
 * first expand, never at launch.
 *
 * Confirming may supersede older contradicting statements: on confirm the
 * server marks the ones this replaces as rejected (consolidatie). We name
 * them for the server but keep the UI to a single act — the server owns the
 * consolidation decision.
 */
import React, { useEffect, useState } from 'react';
import { Check, ChevronRight, Lightbulb, X } from 'lucide-react';
import { cognitionApi } from '../server/api';
import { L } from '../lib/lexicon';

const STATUS_LABEL = {
  proposed: () => L.cognitionStatusProposed,
  testing: () => L.cognitionStatusTesting,
  corroborated: () => L.cognitionStatusCorroborated,
};

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

  const handleConfirm = async (item, e) => {
    e.stopPropagation();
    if (busyId) return;
    setBusyId(item.id);
    setError(null);
    try {
      const response = await cognitionApi.confirm(item.id, { rationale: L.cognitionConfirmed });
      afterAction(item.id, { ...(response?.body?.hypothesis || {}), status: 'confirmed' });
    } catch (_) {
      setError(L.cognitionActionFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleReject = async (item, e) => {
    e.stopPropagation();
    if (busyId) return;
    setBusyId(item.id);
    setError(null);
    try {
      await cognitionApi.reject(item.id, L.cognitionRejected);
      afterAction(item.id, { status: 'rejected' });
    } catch (_) {
      setError(L.cognitionActionFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleTest = async (item, e) => {
    e.stopPropagation();
    if (busyId) return;
    setBusyId(item.id);
    setError(null);
    try {
      const response = await cognitionApi.test(item.id);
      afterAction(item.id, { ...(response?.body || {}), status: 'testing' });
    } catch (_) {
      setError(L.cognitionActionFailed);
    } finally {
      setBusyId(null);
    }
  };

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
                <div key={item.id} className="thread-item cognition-item">
                  <div className="cognition-statement">{item.statement}</div>
                  <div className="cognition-meta">
                    <span className="cognition-status">
                      {(STATUS_LABEL[item.status] || (() => item.status))()}
                    </span>
                  </div>
                  <div className="thread-actions">
                    {item.status !== 'testing' && (
                      <button
                        className="cognition-action"
                        onClick={(e) => handleTest(item, e)}
                        disabled={busyId === item.id}
                        aria-label={L.cognitionTesting}
                        title={L.cognitionTesting}
                      >
                        <Lightbulb size={14} />
                      </button>
                    )}
                    <button
                      className="cognition-action cognition-confirm"
                      onClick={(e) => handleConfirm(item, e)}
                      disabled={busyId === item.id}
                      aria-label={L.cognitionConfirm}
                      title={L.cognitionConfirm}
                    >
                      <Check size={14} />
                    </button>
                    <button
                      className="cognition-action cognition-reject"
                      onClick={(e) => handleReject(item, e)}
                      disabled={busyId === item.id}
                      aria-label={L.cognitionReject}
                      title={L.cognitionReject}
                    >
                      <X size={14} />
                    </button>
                  </div>
                </div>
              ))}
            </>
          )}
        </div>
      )}
    </div>
  );
}
