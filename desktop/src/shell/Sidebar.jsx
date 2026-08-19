import React from 'react';
import { Plus, Settings, Trash2 } from 'lucide-react';
import { L } from '../lib/lexicon';

/**
 * Sidebar — the web's Threads look, ported: brand orb, the italic serif
 * new-page button, the thread list with hover-delete, the NL/EN switcher
 * and the quiet foot lines. The settings button is the desktop's own.
 */
export default function Sidebar({
  threads,
  activeId,
  lang,
  onSelect,
  onNew,
  onDelete,
  onLangChange,
  onOpenSettings,
}) {
  return (
    <nav className="sidebar">
      <div className="brand">
        <span className="brand-mark" />
        <span className="brand-name">Gaia</span>
      </div>

      <button className="new-thread-btn" onClick={onNew}>
        <Plus size={16} /> {L.newPage}
      </button>

      <div className="thread-list">
        {threads.map((t) => (
          <div
            key={t.id}
            className={`thread-item${t.id === activeId ? ' active' : ''}`}
            onClick={() => onSelect(t.id)}
          >
            <span className="thread-title">{t.title || L.untitled}</span>
            <button
              className="thread-delete"
              onClick={(e) => {
                e.stopPropagation();
                onDelete(t.id);
              }}
              aria-label={L.deleteConversation}
            >
              <Trash2 size={14} />
            </button>
          </div>
        ))}
      </div>

      <div className="sidebar-foot">
        <button className="settings-open-btn" onClick={onOpenSettings}>
          <Settings size={13} /> {L.settings}
        </button>
        <div className="lang-switcher">
          <button
            className={`lang-btn ${lang === 'nl' ? 'active' : ''}`}
            onClick={() => onLangChange('nl')}
          >
            NL
          </button>
          <span className="lang-separator">/</span>
          <button
            className={`lang-btn ${lang === 'en' ? 'active' : ''}`}
            onClick={() => onLangChange('en')}
          >
            EN
          </button>
        </div>
        <span className="foot-line">{L.footLine1}</span>
        <span className="foot-line">{L.footLine2}</span>
      </div>
    </nav>
  );
}
