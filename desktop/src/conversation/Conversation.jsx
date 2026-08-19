import React, { useEffect, useMemo, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import MessageView from './MessageView';
import Composer from './Composer';
import Presence from '../presence/Presence';
import { PresenceOrb } from '../presence/PresenceOrb';
import { getGreeting } from '../lib/greeting';

/**
 * Conversation — the web's structure, ported: centered column, the welcome
 * moment (orb + greeting), messages, the thinking row, and the composer
 * dock with Gaia's presence above it.
 */
export default function Conversation({ thread, busy, presenceState, whisper, onSend, onRetry }) {
  const scrollRef = useRef(null);
  const isAtBottomRef = useRef(true);
  const messages = thread?.messages || [];
  const prevLenRef = useRef(messages.length);

  const greeting = useMemo(() => getGreeting(), []);
  const [hasDraft, setHasDraft] = useState(false);

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (messages.length > prevLenRef.current || isAtBottomRef.current) {
      el.scrollTo({ top: el.scrollHeight, behavior: 'smooth' });
    }
    prevLenRef.current = messages.length;
  }, [messages.length, busy]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    isAtBottomRef.current = el.scrollHeight - el.scrollTop - el.clientHeight <= 80;
  };

  const empty = messages.length === 0 && !busy;

  return (
    <section className="conversation">
      <div className="conversation-scroll" ref={scrollRef} onScroll={handleScroll}>
        <div className="conversation-column">
          {empty ? (
            <div className="welcome">
              <motion.div
                initial={{ opacity: 0, scale: 0.85 }}
                animate={{ opacity: 1, scale: 1 }}
                transition={{ duration: 1.4, ease: 'easeOut' }}
              >
                <PresenceOrb state="quiet" size={72} />
              </motion.div>
              <motion.h1
                className="welcome-title"
                initial={{ opacity: 0, y: 12 }}
                animate={{ opacity: 1, y: 0 }}
                transition={{ duration: 1.0, delay: 0.7, ease: 'easeOut' }}
              >
                {greeting.title}
              </motion.h1>
            </div>
          ) : (
            <>
              {messages.map((m) => (
                <MessageView key={m.id} message={m} onRetry={onRetry} />
              ))}
              {busy && (
                <div className="thinking-row">
                  <Presence isThinking={true} type="general" state="thinking" size={26} />
                </div>
              )}
            </>
          )}
        </div>
      </div>

      {whisper && (
        <div className="health-whisper" role="status">
          <PresenceOrb state="quiet" size={18} />
          <span>{whisper}</span>
        </div>
      )}

      <div className="composer-dock">
        <div className="conversation-column">
          <div className="dock-presence">
            <PresenceOrb state={busy ? 'thinking' : hasDraft ? 'listening' : presenceState} size={22} />
          </div>
          <Composer onSend={onSend} busy={busy} onDraftChange={setHasDraft} />
        </div>
      </div>
    </section>
  );
}
