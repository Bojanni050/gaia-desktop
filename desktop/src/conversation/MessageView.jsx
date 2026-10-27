import React, { useRef, useState } from 'react';
import { Check, Copy, Paperclip, RotateCw, Trash2, Undo2 } from 'lucide-react';
import { L } from '../lib/lexicon';
import { formatTurnTime } from '../lib/timestamp';
import Markdown from './Markdown';

function AssistantBody({ content, reasoning }) {
  let cleanContent = content;
  let displayReasoning = reasoning;

  if (!displayReasoning && content && content.includes('<think>')) {
    const thinkStart = content.indexOf('<think>');
    const thinkEnd = content.indexOf('</think>');
    if (thinkEnd === -1) {
      displayReasoning = content.slice(thinkStart + 7);
      cleanContent = content.slice(0, thinkStart);
    } else {
      displayReasoning = content.slice(thinkStart + 7, thinkEnd);
      cleanContent = content.slice(0, thinkStart) + content.slice(thinkEnd + 8);
    }
  }

  return (
    <>
      {displayReasoning && (
        // Closed from the first token: the thought process is something to
        // look into, not a panel that unfolds above the answer while the
        // answer is still being written. An open panel pushes the reply down
        // under the person's eyes mid-sentence and takes the scroll position
        // with it. It opens by hand, whenever they want it.
        <details className="thought-process">
          <summary className="thought-process-title">{L.thoughtProcess}</summary>
          <div className="thought-process-body">
            <Markdown>{displayReasoning}</Markdown>
          </div>
        </details>
      )}
      <Markdown>{cleanContent}</Markdown>
    </>
  );
}

/**
 * MessageView — the web's message anatomy, ported: user bubbles right in
 * accent-soft, assistant replies in Gaia's serif voice under her name.
 *
 * Under Gaia's reply sits the person's own way to take a false memory back.
 * Gaia calling `retract_memory` herself is the judgement that failed when she
 * recited something untrue, so this path never asks her: they point at the
 * sentence, and the server matches it against what is really in memory.
 */
export default function MessageView({ message, onRetry, onDelete, onRetract }) {
  const isUser = message.role === 'user';
  const [copied, setCopied] = useState(false);
  const [retractOpen, setRetractOpen] = useState(false);
  const [quote, setQuote] = useState('');
  const [correction, setCorrection] = useState('');
  const [answer, setAnswer] = useState(null);
  const [working, setWorking] = useState(false);
  const bodyRef = useRef(null);
  const time = formatTurnTime(message.createdAt);

  const copy = () => {
    navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  /**
   * The sentence they marked in THIS reply is the quote — a highlight
   * somewhere else on the page is not, so only a selection inside her answer
   * counts. The button prevents its own mousedown, which is what would
   * otherwise drop the selection before the click is handled.
   */
  const openRetract = () => {
    let selected = '';
    const selection = typeof window !== 'undefined' && window.getSelection ? window.getSelection() : null;
    if (selection && selection.anchorNode && bodyRef.current && bodyRef.current.contains(selection.anchorNode)) {
      selected = String(selection.toString() || '');
    }
    setQuote(selected.replace(/\s+/g, ' ').trim());
    setAnswer(null);
    setRetractOpen(true);
  };

  /**
   * Sends the sentence — or, when the answer was several memories at once,
   * the one they picked. A failure never becomes a raw error on screen: the
   * take-back either answers plainly or says it could not right now.
   */
  const send = async (text) => {
    const sentence = String(text != null ? text : quote).replace(/\s+/g, ' ').trim();
    if (!sentence || working) return;
    setWorking(true);
    try {
      const result = await onRetract(sentence, correction.replace(/\s+/g, ' ').trim());
      setQuote(sentence);
      setAnswer(result);
      setRetractOpen(false);
    } catch (_) {
      setAnswer({ status: 'failed', message: L.retractFailed });
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className={`message ${isUser ? 'user' : 'assistant'}${message.failed ? ' failed' : ''}`}>
      <div className="msg-inner">
        {!isUser && (
          <div className="msg-author">
            Gaia
            {message.kept?.length > 0 && (
              <span
                className="memory-dot"
                role="img"
                aria-label={L.memoryKept}
                title={`${L.memoryKept}: ${message.kept.map((k) => k.text).filter(Boolean).join(' · ')}`}
              />
            )}
          </div>
        )}
        <div className="msg-body" ref={bodyRef}>
          {isUser ? (
            <p className="user-text">{message.content}</p>
          ) : (
            <AssistantBody content={message.content} reasoning={message.reasoning} />
          )}
        </div>

        {time && (
          <time className="msg-time" dateTime={message.createdAt}>
            {time}
          </time>
        )}

        {isUser && message.attachments?.length > 0 && (
          <div className="msg-attachments">
            {message.attachments.map((a) => (
              <span key={a.id} className="attachment-chip attachment-chip-static">
                <Paperclip size={11} />
                {a.filename}
              </span>
            ))}
          </div>
        )}

        <div className="msg-actions">
          {message.content && (
            <button className="msg-action" onClick={copy} aria-label="Copy">
              {copied ? <Check size={13} /> : <Copy size={13} />}
            </button>
          )}
          {!isUser && message.content && !message.failed && onRetry && (
            <button className="msg-action" onClick={() => onRetry(message.id)} aria-label={L.retry} title={L.retry}>
              <RotateCw size={13} />
            </button>
          )}
          {message.failed && onRetry && (
            <button className="msg-action" onClick={() => onRetry(message.id)} aria-label={L.retry}>
              <RotateCw size={13} />
              <span>{L.retry}</span>
            </button>
          )}
          {onDelete && (
            <button className="msg-action" onClick={() => onDelete(message.id)} aria-label="Delete">
              <Trash2 size={13} />
            </button>
          )}
          {!isUser && onRetract && (
            <button
              className="msg-action"
              onClick={openRetract}
              onMouseDown={(e) => e.preventDefault()}
              aria-label={L.retract}
            >
              <Undo2 size={13} />
              <span>{L.retract}</span>
            </button>
          )}
        </div>

        {retractOpen && (
          <div className="retract-panel">
            <p className="retract-hint">{L.retractHint}</p>
            <label className="retract-field">
              <span>{L.retractQuoteLabel}</span>
              <input value={quote} onChange={(e) => setQuote(e.target.value)} />
            </label>
            <label className="retract-field">
              <span>{L.retractCorrectionLabel}</span>
              <input value={correction} onChange={(e) => setCorrection(e.target.value)} />
            </label>
            <div className="retract-actions">
              <button className="msg-action" onClick={() => send()} disabled={working || !quote.trim()}>
                {working ? L.retractWorking : L.retractSend}
              </button>
              <button className="msg-action" onClick={() => setRetractOpen(false)}>
                {L.retractCancel}
              </button>
            </div>
          </div>
        )}

        {answer && (
          <div className="retract-answer" role="status">
            <p>{answer.message}</p>
            {answer.status === 'ambiguous' && Array.isArray(answer.candidates) && answer.candidates.length > 0 && (
              <div className="retract-candidates">
                <p>{L.retractPick}</p>
                {answer.candidates.map((candidate, index) => (
                  <button
                    key={`${candidate.bank}-${candidate.id}-${index}`}
                    className="msg-action"
                    onClick={() => send(candidate.text)}
                    disabled={working}
                  >
                    {candidate.text}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}
