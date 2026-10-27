import React, { useLayoutEffect, useMemo, useRef, useState } from 'react';
import { motion } from 'framer-motion';
import MessageView from './MessageView';
import Composer from './Composer';
import ResearchStatus from './ResearchStatus';
import DesktopPrompt from './DesktopPrompt';
import Presence from '../presence/Presence';
import { stepProgressLabel } from '../state/stepProgress';
import { PresenceOrb } from '../presence/PresenceOrb';
import { getGreeting } from '../lib/greeting';

/**
 * How close to the tail still counts as "at the bottom" (px). Small enough that
 * the person's own scrolling away is noticed, large enough to absorb rounding
 * and line-height leftovers.
 */
const NEAR_BOTTOM = 40;

/**
 * Conversation — the web's structure, ported: centered column, the welcome
 * moment (orb + greeting), messages, the thinking row, and the composer
 * dock with Gaia's presence above it.
 */
export default function Conversation({ thread, busy, streaming, progress, presenceState, listenState, whisper, onSend, onStop, onRetry, onRetract, onCommand, research, onOpenResearchConversation, desktop }) {
  const scrollRef = useRef(null);
  // Whether the view follows the thread's tail. Only the person's own scrolling
  // turns this off — never our own jump to the bottom (see handleScroll).
  const followRef = useRef(true);
  // The height the view was last pinned to. Content that arrived since is not
  // the person's doing and must not read as "scrolled away": a streaming reply
  // grows by tens of pixels between one jump and the next, and counting that as
  // a scroll is exactly what used to stop the view halfway through an answer.
  const pinnedHeightRef = useRef(0);
  const messages = thread?.messages || [];
  const prevLenRef = useRef(messages.length);

  const greeting = useMemo(() => getGreeting(), []);
  const [hasDraft, setHasDraft] = useState(false);

  // A streaming reply grows the last message's content in place rather than
  // adding new messages, so messages.length alone won't re-run this effect
  // while it's arriving — the last message's length stands in for "did the
  // visible content just grow" during a stream.
  const lastContentLength = messages[messages.length - 1]?.content?.length || 0;

  // Opening another conversation shows its newest turn.
  useLayoutEffect(() => {
    followRef.current = true;
    pinnedHeightRef.current = 0;
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [thread?.id]);

  // Laid out in the same frame as the text it is reacting to (useLayoutEffect,
  // not useEffect): jumping after the browser painted the new text is what makes
  // a growing reply look like it is one step ahead of the view. The jump is
  // instant rather than smooth for the same reason — an animation that takes
  // 300 ms cannot keep up with deltas arriving every few milliseconds, so it
  // would chase the reply and never arrive.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    // A new message always brings the tail into view (someone who sent it is
    // done reading the older part); a reply merely growing only follows while
    // the person is still at the tail.
    if (messages.length > prevLenRef.current) followRef.current = true;
    prevLenRef.current = messages.length;
    if (!followRef.current) return;
    el.scrollTo({ top: el.scrollHeight, behavior: 'auto' });
    pinnedHeightRef.current = el.scrollHeight;
  }, [messages.length, busy, lastContentLength, streaming]);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distance <= NEAR_BOTTOM) {
      followRef.current = true;
      return;
    }
    // Our own jump fires a scroll event too, and by the time that event is
    // handled the reply may already have grown the thread again — so its growth
    // shows up in `distance` although nobody touched the scrollbar. Only the
    // movement beyond that growth is the person's own, and only that decides
    // whether following stops.
    const growth = Math.max(0, el.scrollHeight - pinnedHeightRef.current);
    if (distance - growth > NEAR_BOTTOM) followRef.current = false;
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
                <MessageView key={m.id} message={m} onRetry={onRetry} onRetract={onRetract} />
              ))}
              {busy && !streaming && (
                // The pulsing orb while Gaia works on her reply, with no sequenced thinking
                // words. Only a multi-step plan adds its step line ("Step 2 of 3 - ..."): that
                // is concrete, honest information about what she is doing right now. Progress
                // is a separate frame from the answer, so this row disappears the moment the
                // reply itself starts streaming in.
                <div className="thinking-row">
                  <Presence state="thinking" size={36} label={progress ? stepProgressLabel(progress) : undefined} />
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

      {research && (
        <ResearchStatus
          running={research.running}
          awaiting={research.awaiting}
          finished={research.finished}
          activeConversationId={thread?.id}
          onAnswer={research.answer}
          onOpen={onOpenResearchConversation}
        />
      )}

      {desktop && (
        <DesktopPrompt
          prompts={desktop.prompts}
          notice={desktop.notice}
          onDecide={desktop.controller.decide}
          onDismissNotice={desktop.controller.dismissNotice}
        />
      )}

      <div className="composer-dock">
        <div className="conversation-column">
          <div className="dock-presence">
            <PresenceOrb state={busy ? 'thinking' : hasDraft ? 'listening' : presenceState} size={30} />
          </div>
          <Composer onSend={onSend} onStop={onStop} busy={busy} onDraftChange={setHasDraft} listenState={listenState} onCommand={onCommand} />
        </div>
      </div>
    </section>
  );
}
