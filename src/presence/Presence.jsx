import React from 'react';
import { AnimatePresence, motion } from 'framer-motion';
import { PresenceOrb } from './PresenceOrb';
import { PresenceMessage } from './PresenceMessage';
import { usePresenceAnimator } from './usePresenceAnimator';

/**
 * Composite Presence component — ported from Gaia Web.
 * Renders the orb and, while thinking, a sequenced calm message.
 */
export default function Presence({ isThinking = false, type = 'general', state = 'quiet', size = 44, label }) {
  const animatedMessage = usePresenceAnimator(isThinking, type);

  return (
    <div
      className="presence"
      data-state={state}
      title={`Gaia — ${state}`}
      style={{ display: 'flex', alignItems: 'center', gap: '12px' }}
    >
      <PresenceOrb state={state} size={size} />
      {label && <span className="presence-label">{label}</span>}
      <AnimatePresence>
        {isThinking && (
          <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.8 }}>
            <PresenceMessage message={animatedMessage} />
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
