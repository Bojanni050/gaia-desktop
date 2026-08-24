import React, { useState } from 'react';
import { Check, Copy, Paperclip, RotateCw, Trash2 } from 'lucide-react';
import { L } from '../lib/lexicon';
import Markdown from './Markdown';

function AssistantBody({ content, reasoning, streaming }) {
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
        <details className="thought-process" open={streaming}>
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
 */
export default function MessageView({ message, streaming, onRetry, onDelete }) {
  const isUser = message.role === 'user';
  const [copied, setCopied] = useState(false);

  const copy = () => {
    navigator.clipboard.writeText(message.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 1400);
  };

  return (
    <div className={`message ${isUser ? 'user' : 'assistant'}${message.failed ? ' failed' : ''}`}>
      <div className="msg-inner">
        {!isUser && <div className="msg-author">Gaia</div>}
        <div className="msg-body">
          {isUser ? (
            <p className="user-text">{message.content}</p>
          ) : (
            <AssistantBody content={message.content} reasoning={message.reasoning} streaming={streaming} />
          )}
        </div>

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
          {message.failed && onRetry && (
            <button className="msg-action" onClick={() => onRetry(message.id)} aria-label="Retry">
              <RotateCw size={13} />
              <span>{L.retry}</span>
            </button>
          )}
          {onDelete && (
            <button className="msg-action" onClick={() => onDelete(message.id)} aria-label="Delete">
              <Trash2 size={13} />
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
