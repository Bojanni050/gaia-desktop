import React from 'react';
import { motion, AnimatePresence } from 'framer-motion';
import { L } from '../lib/lexicon';

/**
 * ConnectionNotice — startup offline pop.
 *
 * Shown once on launch when the Gaia Cloud link is not online
 * (offline / unauthorized / notConfigured). Fades in calmly and
 * asks the user to check credentials, with a shortcut to Settings.
 */
export default function ConnectionNotice({ open, status, onOpenSettings, onDismiss }) {
  return (
    <AnimatePresence>
      {open && (
        <motion.div
          className="connection-notice-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          transition={{ duration: 0.45, ease: 'easeOut' }}
          onClick={onDismiss}
        >
          <motion.div
            className="connection-notice"
            role="alertdialog"
            aria-live="assertive"
            aria-label={L.connectionTitle}
            initial={{ opacity: 0, scale: 0.96, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.97, y: 6 }}
            transition={{ duration: 0.45, ease: 'easeOut' }}
            onClick={(e) => e.stopPropagation()}
          >
            <h2 className="connection-notice-title">{L.connectionTitle}</h2>
            <p className="connection-notice-message">
              {status === 'unauthorized' ? L.turnUnauthorized : L.connectionMessage}
            </p>
            <div className="connection-notice-actions">
              <button type="button" className="primary" onClick={onOpenSettings}>
                {L.connectionOpenSettings}
              </button>
              <button type="button" onClick={onDismiss}>
                {L.connectionDismiss}
              </button>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
