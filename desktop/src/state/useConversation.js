/**
 * Conversation state — turns over the Gaia Cloud seam, in-memory threads.
 *
 * This hook owns no cognition: it sends user text through serverApi and
 * appends whatever reply the server returns. SOUL, memory, intent and
 * reasoning never run here. The one thing added on top of that reply —
 * speaking it via speechApi/playSpeech, gated by lib/language.js's
 * looksEnglish (Xiaomi's TTS model only pronounces Chinese/English) — is
 * presentation, not cognition: it runs strictly after a reply already
 * exists, never changes what was said, and its own failure can never
 * affect the turn (see runTurn).
 */
import { useCallback, useState } from 'react';
import { buildStreamTurnBody } from './contract';
import { phraseTurnError } from './phrases';
import { speechApi } from '../server/api';
import { playSpeech } from '../lib/speech';
import { looksEnglish } from '../lib/language';

let counter = 1;
const localId = () => `${Date.now()}-${counter++}`;

export function useConversation(server) {
  const [threads, setThreads] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [busy, setBusy] = useState(false);
  // True once the assistant's reply has started arriving — distinct from
  // `busy` (which also covers the pre-first-token wait) so Conversation.jsx
  // can swap the thinking indicator for the live-updating message at the
  // right moment, not a beat too early or too late.
  const [streaming, setStreaming] = useState(false);

  const active = threads.find((t) => t.id === activeId) || null;

  const newThread = useCallback(() => {
    const thread = { id: localId(), title: null, messages: [] };
    setThreads((prev) => [thread, ...prev]);
    setActiveId(thread.id);
  }, []);

  const openThread = useCallback((id) => {
    setActiveId(id);
  }, []);

  /**
   * Loads a conversation the History panel fetched from Gaia Cloud
   * (historyApi.get) into the active thread list, keyed by the same id
   * the server already knows it by — so continuing the conversation from
   * here appends to the same saved transcript rather than starting a new
   * one. Replaces the thread if it's already open (e.g. re-opening after
   * it fell out of the in-session list).
   */
  const hydrateThread = useCallback((id, messages) => {
    const firstUser = messages.find((m) => m.role === 'user');
    const thread = {
      id,
      title: firstUser ? firstUser.content.slice(0, 48) : null,
      messages: messages.map((m) => ({ ...m, id: localId() })),
    };
    setThreads((prev) => {
      const exists = prev.some((t) => t.id === id);
      return exists ? prev.map((t) => (t.id === id ? thread : t)) : [thread, ...prev];
    });
    setActiveId(id);
  }, []);

  const deleteThread = useCallback((id) => {
    setThreads((prev) => {
      const next = prev.filter((t) => t.id !== id);
      setActiveId((current) => (current === id ? next[0]?.id ?? null : current));
      return next;
    });
  }, []);

  /**
   * Perform a turn for an already-built history, streaming the reply in as
   * it arrives. The assistant message is created empty on the first delta
   * (not before — until then the thinking indicator is still the honest
   * state) and grown in place from there.
   */
  const runTurn = useCallback(
    async (threadId, history) => {
      setBusy(true);
      const assistantId = localId();
      // Tracked outside the state updaters below (which must stay pure —
      // React StrictMode double-invokes them in dev, so mutating a closure
      // variable from inside one would drop or duplicate the first delta).
      // "does the thread already have this message" is instead derived
      // fresh from `prev` each time, making both updaters idempotent.
      let receivedAny = false;

      let fullReasoning = '';
      const appendToAssistant = (text) => {
        if (!text) return;
        receivedAny = true;
        setStreaming(true);
        setThreads((prev) =>
          prev.map((t) => {
            if (t.id !== threadId) return t;
            const exists = t.messages.some((m) => m.id === assistantId);
            if (!exists) {
              return { ...t, messages: [...t.messages, { id: assistantId, role: 'assistant', content: text }] };
            }
            return {
              ...t,
              messages: t.messages.map((m) =>
                m.id === assistantId ? { ...m, content: m.content + text } : m
              ),
            };
          })
        );
      };

      try {
        // threadId doubles as the server's conversationId — Gaia Cloud
        // saves/appends the transcript under it (conversationStore.js),
        // which is what lets History reopen this same thread later.
        const body = buildStreamTurnBody(history, threadId);
        const fullReply = await server.streamTurn(body, (delta) => {
          if (delta.reasoningContent) fullReasoning += delta.reasoningContent;
          appendToAssistant(delta.content);
        });
        // A turn that streamed no content deltas at all (empty reply) is a
        // server-contract violation, same as the old parseReply's check —
        // surface it as a failure rather than leaving a blank bubble.
        if (!receivedAny || !fullReply) {
          throw new Error('Gaia Server returned no reply');
        }
        if (fullReasoning) {
          setThreads((prev) =>
            prev.map((t) => {
              if (t.id !== threadId) return t;
              return {
                ...t,
                messages: t.messages.map((m) =>
                  m.id === assistantId ? { ...m, reasoning: fullReasoning } : m
                ),
              };
            })
          );
        }
        // Gaia's voice — strictly a presentation step on the reply that
        // just arrived. Fire-and-forget: text is already the canonical,
        // already-displayed Gaia response; a TTS failure (not configured,
        // provider hiccup, playback denied) must never affect the turn
        // that already succeeded, so it's caught and swallowed here, same
        // posture as reflectOnTurn/history-save on the server side.
        //
        // Gated to English-looking replies only: Xiaomi's
        // mimo-v2.5-tts-voicedesign only supports Chinese/English
        // pronunciation — Dutch text comes back badly mispronounced
        // rather than rejected, so this must be checked before ever
        // calling speechApi (see lib/language.js's own comment).
        if (looksEnglish(fullReply)) {
          speechApi
            .synthesize(fullReply)
            .then((bytes) => playSpeech(bytes))
            .catch(() => {});
        }
      } catch (error) {
        const phrase = phraseTurnError(error);
        setThreads((prev) =>
          prev.map((t) => {
            if (t.id !== threadId) return t;
            const exists = t.messages.some((m) => m.id === assistantId);
            if (exists) {
              return {
                ...t,
                messages: t.messages.map((m) =>
                  m.id === assistantId ? { ...m, content: phrase, failed: true } : m
                ),
              };
            }
            return { ...t, messages: [...t.messages, { id: assistantId, role: 'assistant', content: phrase, failed: true }] };
          })
        );
      } finally {
        setBusy(false);
        setStreaming(false);
      }
    },
    [server]
  );

  const send = useCallback(
    (rawText, attachments = []) => {
      const content = rawText.trim();
      if (!content || busy) return;

      const base = active || { id: localId(), title: null, messages: [] };
      const threadId = base.id;
      // `attachments` (full metadata) is kept for rendering the chip in
      // MessageView; `attachmentIds` is the only part buildTurnRequest
      // actually sends on — file bytes never pass through this hook, they
      // already reached the library at upload time.
      const userMessage = {
        id: localId(),
        role: 'user',
        content,
        attachments,
        attachmentIds: attachments.map((a) => a.id),
      };
      const history = [...base.messages, userMessage];

      setThreads((prev) => {
        const exists = prev.some((t) => t.id === threadId);
        const thread = {
          id: threadId,
          title: base.title || content.slice(0, 48),
          messages: history,
        };
        return exists ? prev.map((t) => (t.id === threadId ? thread : t)) : [thread, ...prev];
      });
      setActiveId(threadId);
      return runTurn(threadId, history);
    },
    [active, busy, runTurn]
  );

  /** Retry a failed assistant message: drop it and everything after, resend. */
  const retry = useCallback(
    (messageId) => {
      const thread = threads.find((t) => t.id === activeId);
      if (!thread || busy) return;
      const index = thread.messages.findIndex((m) => m.id === messageId);
      if (index === -1) return;
      const history = thread.messages.slice(0, index);
      if (!history.some((m) => m.role === 'user')) return;

      setThreads((prev) => prev.map((t) => (t.id === thread.id ? { ...t, messages: history } : t)));
      return runTurn(thread.id, history);
    },
    [threads, activeId, busy, runTurn]
  );

  return { threads, active, activeId, busy, streaming, newThread, openThread, deleteThread, hydrateThread, send, retry };
}
