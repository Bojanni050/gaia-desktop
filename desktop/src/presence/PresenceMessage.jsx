import React from 'react';
import { motion, AnimatePresence } from 'framer-motion';

export function PresenceMessage({ message }) {
  return (
    <div
      className="presence-message-container"
      style={{ position: 'relative', height: '1.5em', display: 'flex', alignItems: 'center' }}
    >
      <AnimatePresence mode="wait">
        <motion.span
          key={message}
          initial={{ opacity: 0, y: 2 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -2 }}
          transition={{ duration: 0.8, ease: 'easeInOut' }}
          className="thinking-word"
          style={{ position: 'absolute', whiteSpace: 'nowrap' }}
        >
          {message}
        </motion.span>
      </AnimatePresence>
    </div>
  );
}
