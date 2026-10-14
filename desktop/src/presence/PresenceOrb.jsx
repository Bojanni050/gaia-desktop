/**
 * Presence Orb — ported verbatim from Gaia Web (framer-motion timings
 * included), so Gaia breathes identically in both clients.
 *
 * The orb is not decoration — it is Gaia being "here". Four states, each a
 * slow, almost meditative breath. Transitions between states are smooth,
 * never flashy. Presence communicates readiness and attention, it never
 * entertains.
 */
import React from 'react';
import { motion } from 'framer-motion';

const STATES = {
  quiet: { scale: [1, 1.03, 1], coreOpacity: [0.48, 0.62, 0.48], halo: 0.20, breath: 8.0 },
  listening: { scale: [1, 1.05, 1], coreOpacity: [0.60, 0.80, 0.60], halo: 0.34, breath: 5.2 },
  // Thinking is the one state that has to be readable at a glance: Gaia is
  // working and the person waiting should not have to look twice. So this
  // breath swings wider, reaches full brightness and carries a stronger halo -
  // while staying SLOWER than speech, so the pulse still reads as composing
  // rather than as talking.
  thinking: { scale: [1, 1.11, 1], coreOpacity: [0.72, 1.00, 0.72], halo: 0.60, breath: 3.0 },
  speaking: { scale: [1, 1.06, 1], coreOpacity: [0.80, 1.00, 0.80], halo: 0.62, breath: 2.4 },
};

export { STATES };

const EASE = [0.37, 0, 0.63, 1]; // slow sinusoidal in/out — meditative

export function PresenceOrb({ state = 'quiet', size = 44 }) {
  const s = STATES[state] || STATES.quiet;
  return (
    <div className="presence-orb-wrap" style={{ width: size, height: size }} data-state={state} title={`Gaia — ${state}`}>
      {/* Halo — intensity eases smoothly when the state changes */}
      <motion.span
        className="presence-halo"
        animate={{ scale: s.scale, opacity: s.halo }}
        transition={{
          scale: { duration: s.breath, repeat: Infinity, ease: EASE },
          opacity: { duration: 1.6, ease: 'easeInOut' },
        }}
      />
      {/* A second, slower halo drift keeps presence feeling alive without motion noise */}
      <motion.span
        className="presence-halo faint"
        animate={{ rotate: 360 }}
        transition={{ duration: 46, repeat: Infinity, ease: 'linear' }}
      />
      <motion.span
        className="presence-orb"
        animate={{ scale: s.scale, opacity: s.coreOpacity }}
        transition={{ duration: s.breath, repeat: Infinity, ease: EASE }}
      />
    </div>
  );
}
