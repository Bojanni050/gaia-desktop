/**
 * Conversation state — turns over the Gaia Cloud seam, in-memory threads.
 *
 * This hook owns no cognition: it sends user text through serverApi and
 * appends whatever reply the server returns. SOUL, memory, intent and
 * reasoning never run here. The one thing added on top of that reply —
 * speaking it via playSpeech (synthesis and playback in Rust), gated by lib/language.js's
 * shouldSpeak over the voice GET /speech/info describes (Dutch only when
 * the voice pronounces it — Xiaomi's TTS model only manages
 * Chinese/English) — is presentation, not cognition: it runs strictly
 * after a reply already exists, never changes what was said, and its own
 * failure can never affect the turn (see runTurn).
 */
import { useCallback, useRef, useState } from 'react';
import { buildStreamTurnBody } from './contract';
import { phraseTurnError } from './phrases';
import { getSpeechInfo } from '../server/api';
import { playSpeech, speaksReplies, stopSpeech } from '../lib/speech';
import { shouldSpeak } from '../lib/language';

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
  // Plan progress (Gaia Server's responseEngine sends each step of a
  // multi-step plan as a `step` frame): which step is running right now.
  // Presentation state only — it never enters the transcript, it clears the
  // moment content starts arriving (the live message is the more specific
  // thing to show by then), and `finally` clears it so a failed turn can
  // never leave a stale "Step 2 of 3" hanging over the thread.
  const [progress, setProgress] = useState(null);
  // The turn in flight, if any. A turn that was cancelled (the user interrupted) may finish after a new
  // one has started; only the current turn is allowed to touch the busy/streaming state.
  const currentTurn = useRef(null);

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
      // A new turn supersedes whatever Gaia was still saying: cut the
      // previous reply off the moment a new one is asked for, rather than
      // letting her talk over the answer she is about to give. Playback is
      // presentation-only, so this never touches the turn itself.
      stopSpeech();
      const turn = { requestId: null, cancelled: false };
      currentTurn.current = turn;
      setBusy(true);
      const assistantId = localId();
      // Stamped the moment the reply first exists — the turn time is when the
      // answer was actually said, not whenever the next turn happens to
      // re-save the transcript. Held in a closure (not state) so the same
      // value rides every streaming delta.
      let assistantCreatedAt = null;
      // Tracked outside the state updaters below (which must stay pure —
      // React StrictMode double-invokes them in dev, so mutating a closure
      // variable from inside one would drop or duplicate the first delta).
      // "does the thread already have this message" is instead derived
      // fresh from `prev` each time, making both updaters idempotent.
      let receivedAny = false;
      // Gaia Server's calm failure wording, when it reported one on the
      // already-open stream (`error` frame). Kept only so the failure
      // below is reported for the reason it actually happened — the phrase
      // the user sees is still phrases.js's own calm wording.
      let serverFailure = null;

      let fullReasoning = '';
      const appendToAssistant = (text) => {
        if (!text) return;
        receivedAny = true;
        if (!assistantCreatedAt) assistantCreatedAt = new Date().toISOString();
        setStreaming(true);
        setThreads((prev) =>
          prev.map((t) => {
            if (t.id !== threadId) return t;
            const exists = t.messages.some((m) => m.id === assistantId);
            if (!exists) {
              return { ...t, messages: [...t.messages, { id: assistantId, role: 'assistant', content: text, createdAt: assistantCreatedAt }] };
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
          // Deltas already on their way when the turn was cancelled are dropped.
          if (turn.cancelled) return;
          // Plan progress rides its OWN frame and is never content: show
          // it while that step runs, drop it the instant the step ends
          // (the next step's `start` — or the reply itself — takes over).
          if (delta.step) {
            setProgress(delta.step.status === 'start' ? delta.step : null);
            return;
          }
          if (delta.error) serverFailure = delta.error;
          // Content has begun: the growing message now says more than any
          // "Step 2 of 3" line could.
          setProgress(null);
          if (delta.reasoningContent) fullReasoning += delta.reasoningContent;
          appendToAssistant(delta.content);
        }, turn);
        // Interrupted while the last bytes arrived: what is on screen stays, nothing is spoken.
        if (turn.cancelled) return;
        // A turn that streamed no content deltas at all (empty reply) must
        // surface as a failure rather than leaving a blank bubble — either
        // the server reported the failure on the stream itself (calm
        // `error` frame, once progress had already opened it) or the reply
        // never arrived at all.
        if (!receivedAny || !fullReply) {
          throw new Error(serverFailure || 'Gaia Server returned no reply');
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
        // Gated by what the configured voice pronounces (GET /speech/info,
        // read from the provider — never judged from the reply): a Dutch-
        // speaking voice (Mistral Voxtral) speaks any reply, while a
        // Chinese/English-only one (Xiaomi voicedesign) only gets English-
        // looking text — Dutch comes back badly mispronounced rather than
        // rejected, so that must be checked before ever calling
        // speechApi (see lib/language.js's own comment). An unreachable
        // or older server answers no info at all, and the gate falls back
        // to the legacy English-only behavior: staying quiet is the safe
        // failure mode.
        (async () => {
          try {
            // Switched off in Settings: no request of any kind, so no speech provider is called.
            if (!speaksReplies()) return;
            const info = await getSpeechInfo(server.request);
            if (!shouldSpeak(fullReply, info && info.languages)) return;
            // Synthesis and playback both happen in Rust (speech_play).
            await playSpeech(fullReply);
          } catch (error) {
            // Never affects the turn — text is already canonical and shown.
            // But don't hide it either: a silent swallow is exactly what once
            // concealed a CSP-blocked blob URL (see tauri.conf.json's csp).
            console.warn('[speech] playback failed:', error);
          }
        })();
      } catch (error) {
        // An interruption is not a failure: the text received so far stays, with no error bubble.
        if (turn.cancelled) return;
        const phrase = phraseTurnError(error);
        if (!assistantCreatedAt) assistantCreatedAt = new Date().toISOString();
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
            return { ...t, messages: [...t.messages, { id: assistantId, role: 'assistant', content: phrase, failed: true, createdAt: assistantCreatedAt }] };
          })
        );
      } finally {
        if (currentTurn.current === turn) {
          currentTurn.current = null;
          setBusy(false);
          setStreaming(false);
          setProgress(null);
        }
      }
    },
    [server]
  );

  /**
   * Stops the turn in flight (the user interrupted Gaia). The connection is closed through Rust so
   * Gaia Cloud can stop generating; the reply received so far stays in the thread. Returns whether a
   * turn was running.
   */
  const cancel = useCallback(() => {
    const turn = currentTurn.current;
    if (!turn || turn.cancelled) return false;
    turn.cancelled = true;
    currentTurn.current = null;
    if (turn.requestId && server.cancelTurn) {
      Promise.resolve(server.cancelTurn(turn.requestId)).catch(() => {});
    }
    setBusy(false);
    setStreaming(false);
    setProgress(null);
    return true;
  }, [server]);

  const send = useCallback(
    (rawText, attachments = [], options = {}) => {
      const content = rawText.trim();
      if (!content || (busy && !options.interrupt)) return;
      // Spoken over a running turn: the new question replaces it.
      if (options.interrupt) cancel();

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
        createdAt: new Date().toISOString(),
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
    [active, busy, runTurn, cancel]
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

  return { threads, active, activeId, busy, streaming, progress, newThread, openThread, deleteThread, hydrateThread, send, retry, cancel };
}
