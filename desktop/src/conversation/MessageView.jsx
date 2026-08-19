import React, { useState } from 'react';
import { Check, Copy, RotateCw, Trash2 } from 'lucide-react';
import { L } from '../lib/lexicon';

/**
 * MessageView — the web's message anatomy, ported: user bubbles right in
 * accent-soft, assistant replies in Gaia's serif voice under her name.
 */
export default function MessageView({ message, onRetry, onDelete }) {
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
            message.content
          )}
        </div>

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
