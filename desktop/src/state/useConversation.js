/**
 * Conversation state — turns over the Gaia Cloud seam, in-memory threads.
 *
 * This hook owns no cognition: it sends user text through serverApi and
 * appends whatever reply the server returns. SOUL, memory, intent and
 * reasoning never run here.
 */
import { useCallback, useState } from 'react';
import { buildTurnRequest, parseReply } from './contract';
import { phraseTurnError } from './phrases';

let counter = 1;
const localId = () => `${Date.now()}-${counter++}`;

export function useConversation(server) {
  const [threads, setThreads] = useState([]);
  const [activeId, setActiveId] = useState(null);
  const [busy, setBusy] = useState(false);

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

  /** Perform a turn for an already-built history and append the reply. */
  const runTurn = useCallback(
    async (threadId, history) => {
      setBusy(true);
      try {
        // threadId doubles as the server's conversationId — Gaia Cloud
        // saves/appends the transcript under it (conversationStore.js),
        // which is what lets History reopen this same thread later.
        const response = await server.request(buildTurnRequest(history, threadId));
        const reply = parseReply(response);
        setThreads((prev) =>
          prev.map((t) =>
            t.id === threadId
              ? { ...t, messages: [...t.messages, { id: localId(), role: 'assistant', content: reply }] }
              : t
          )
        );
      } catch (error) {
        const phrase = phraseTurnError(error);
        setThreads((prev) =>
          prev.map((t) =>
            t.id === threadId
              ? { ...t, messages: [...t.messages, { id: localId(), role: 'assistant', content: phrase, failed: true }] }
              : t
          )
        );
      } finally {
        setBusy(false);
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

  return { threads, active, activeId, busy, newThread, openThread, deleteThread, hydrateThread, send, retry };
}
