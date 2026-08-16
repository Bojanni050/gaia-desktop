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

  const send = useCallback(
    async (rawText) => {
      const content = rawText.trim();
      if (!content || busy) return;

      const base = active || { id: localId(), title: null, messages: [] };
      const threadId = base.id;
      const userMessage = { id: localId(), role: 'user', content };
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
      setBusy(true);

      try {
        const response = await server.request(buildTurnRequest(history));
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
    [active, busy, server]
  );

  return { threads, active, activeId, busy, newThread, openThread, send };
}
