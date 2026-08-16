import React from 'react';
import PresenceOrb, { PRESENCE_LABELS } from '../presence/PresenceOrb';

export default function PresenceBar({ presence, serverUrl }) {
  return (
    <header className="presence-bar">
      <PresenceOrb presence={presence} />
      <span className="presence-label">{PRESENCE_LABELS[presence] || presence}</span>
      {serverUrl && <span className="presence-server">{serverUrl}</span>}
    </header>
  );
}
