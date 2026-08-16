import React from 'react';
import PresenceOrb, { PRESENCE_LABELS } from '../presence/PresenceOrb';

export default function Sidebar({
  threads,
  activeId,
  presence,
  onSelect,
  onNew,
  onOpenSettings,
  onToggleQuiet,
}) {
  return (
    <nav className="sidebar">
      <div className="brand">
        <span className="brand-mark" />
        <span className="brand-name">Gaia</span>
      </div>

      <button className="new-thread-btn" onClick={onNew} data-testid="new-thread">
        New conversation
      </button>

      <div className="thread-list">
        {threads.map((thread) => (
          <div
            key={thread.id}
            className={`thread-item${thread.id === activeId ? ' active' : ''}`}
            onClick={() => onSelect(thread.id)}
          >
            <span className="thread-title">{thread.title || 'Untitled'}</span>
          </div>
        ))}
      </div>

      <div className="sidebar-foot">
        <div className="presence-chip" onClick={onToggleQuiet} title="Toggle quiet mode">
          <PresenceOrb presence={presence} />
          <span className="presence-chip-label">{PRESENCE_LABELS[presence] || presence}</span>
        </div>
        <button className="settings-btn" onClick={onOpenSettings} data-testid="open-settings">
          Settings
        </button>
      </div>
    </nav>
  );
}
