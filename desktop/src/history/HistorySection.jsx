/**
 * History — conversations Gaia Cloud has already saved, across every past
 * session (never just this device's memory of them — see
 * conversationStore.js). Lives inline in the sidebar as a collapsible
 * section, list loaded lazily on first expand rather than on every app
 * launch. Opening one loads its full transcript and hands it to the active
 * thread list (useConversation's hydrateThread), keyed by the same id the
 * server already knows it by, so continuing the conversation appends to the
 * same saved transcript.
 *
 * `refreshToken` changes whenever App.jsx hears a
 * 'conversation.history.changed' server event (another client saved or
 * deleted a conversation) — this re-fetches the list to match, but only if
 * it was already loaded once; it never auto-expands the section or opens a
 * conversation on its own.
 */
import React, { useEffect, useState } from 'react';
import { ChevronRight, History, Trash2 } from 'lucide-react';
import { historyApi } from '../server/api';
import { L } from '../lib/lexicon';

export default function HistorySection({ onOpenConversation, refreshToken }) {
  const [open, setOpen] = useState(false);
  const [conversations, setConversations] = useState(null); // null = not loaded yet
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);

  useEffect(() => {
    if (conversations === null) return; // not loaded yet — first expand will fetch
    historyApi.list().then(setConversations).catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [refreshToken]);

  const toggle = () => {
    const next = !open;
    setOpen(next);
    if (next && conversations === null) {
      historyApi
        .list()
        .then(setConversations)
        .catch(() => setConversations([]));
    }
  };

  const handleOpen = async (conv) => {
    if (busyId) return;
    setBusyId(conv.id);
    setError(null);
    try {
      const { messages } = await historyApi.get(conv.id);
      onOpenConversation(conv.id, messages);
    } catch (_) {
      setError(L.historyOpenFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleDelete = async (conv, e) => {
    e.stopPropagation();
    if (busyId) return;
    setBusyId(conv.id);
    setError(null);
    try {
      await historyApi.remove(conv.id);
      setConversations((prev) => (prev || []).filter((c) => c.id !== conv.id));
    } catch (_) {
      setError(L.historyDeleteFailed);
    } finally {
      setBusyId(null);
    }
  };

  return (
    <div className="sidebar-section">
      <button className="sidebar-section-header" onClick={toggle} aria-expanded={open}>
        <ChevronRight size={13} className={`sidebar-section-chevron${open ? ' open' : ''}`} />
        <History size={13} />
        <span>{L.history}</span>
      </button>

      {open && (
        <div className="sidebar-section-body">
          {error && <div className="sidebar-section-error">{error}</div>}
          {conversations === null ? (
            <p className="sidebar-section-hint">…</p>
          ) : conversations.length === 0 ? (
            <p className="sidebar-section-hint">{L.historyEmpty}</p>
          ) : (
            conversations.map((conv) => (
              <div
                key={conv.id}
                className="thread-item"
                onClick={() => handleOpen(conv)}
                aria-disabled={busyId === conv.id}
              >
                <span className="thread-title">{conv.title || L.untitled}</span>
                <button
                  className="thread-delete"
                  onClick={(e) => handleDelete(conv, e)}
                  disabled={busyId === conv.id}
                  aria-label={L.historyDelete}
                >
                  <Trash2 size={14} />
                </button>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
