/**
 * History — conversations Gaia Cloud has already saved, across every
 * past session (never just this device's memory of them — see
 * conversationStore.js). Opening one loads its full transcript and hands
 * it to the active thread list (useConversation's hydrateThread), keyed
 * by the same id the server already knows it by, so continuing the
 * conversation appends to the same saved transcript. Same calm-overlay
 * pattern as Settings/Library — no confirmation on delete, matching the
 * app's existing convention.
 */
import React, { useEffect, useState } from 'react';
import { MessageSquare, Trash2 } from 'lucide-react';
import { historyApi } from '../server/api';
import { L } from '../lib/lexicon';

function formatDate(iso) {
  try {
    return new Date(iso).toLocaleString(undefined, { year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
  } catch (_) {
    return '';
  }
}

export default function HistoryPanel({ onClose, onOpenConversation }) {
  const [conversations, setConversations] = useState(null);
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);

  const load = () => {
    historyApi
      .list()
      .then(setConversations)
      .catch(() => setConversations([]));
  };

  useEffect(load, []);

  const handleOpen = async (conv) => {
    setBusyId(conv.id);
    setError(null);
    try {
      const { messages } = await historyApi.get(conv.id);
      onOpenConversation(conv.id, messages);
      onClose();
    } catch (_) {
      setError(L.historyOpenFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleDelete = async (conv, e) => {
    e.stopPropagation();
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
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel library-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.historyTitle}</h2>
        <p className="library-hint">{L.historyHint}</p>

        {error && <div className="test-result test-error">{error}</div>}

        {conversations === null ? (
          <p className="library-hint">…</p>
        ) : conversations.length === 0 ? (
          <p className="library-empty">{L.historyEmpty}</p>
        ) : (
          <div className="library-list">
            {conversations.map((conv) => (
              <div key={conv.id} className="library-item history-item" onClick={() => handleOpen(conv)}>
                <div className="library-item-info">
                  <span className="library-item-name">
                    <MessageSquare size={12} /> {conv.title || L.untitled}
                  </span>
                  <span className="library-item-meta">
                    {conv.messageCount} {L.historyMessages} · {formatDate(conv.updatedAt)}
                  </span>
                </div>
                <div className="library-item-actions">
                  <button
                    className="library-icon-btn"
                    onClick={(e) => handleDelete(conv, e)}
                    disabled={busyId === conv.id}
                    aria-label={L.historyDelete}
                    title={L.historyDelete}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <div className="settings-actions">
          <button onClick={onClose}>{L.settingsClose}</button>
        </div>
      </div>
    </div>
  );
}
