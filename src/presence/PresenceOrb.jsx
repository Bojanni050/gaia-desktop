/**
 * Local presence — Gaia on this device. Derived from the server link's
 * reachability plus the user's quiet preference, exactly as the Rust
 * presence controller defines it. The orb breathes; it never flashes.
 */
import React from 'react';

export const PRESENCE_LABELS = {
  localOnly: 'present',
  connecting: 'reaching out',
  available: 'available',
  disconnected: 'beyond reach',
  quiet: 'quiet',
};

export function mapConnectionToPresence(status) {
  switch (status) {
    case 'online':
      return 'available';
    case 'connecting':
      return 'connecting';
    case 'offline':
      return 'disconnected';
    default:
      return 'localOnly';
  }
}

export default function PresenceOrb({ presence }) {
  return <span className={`orb orb-${presence}`} aria-label={PRESENCE_LABELS[presence] || presence} />;
}
