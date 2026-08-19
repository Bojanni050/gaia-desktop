import React, { useRef, useState } from 'react';
import { ArrowUp, Paperclip, X } from 'lucide-react';
import { libraryApi } from '../server/api';
import { L } from '../lib/lexicon';

/**
 * Composer — the web's pill, ported: Enter sends, Shift+Enter is a newline,
 * focus gathers a soft accent ring.
 *
 * Attaching a file uploads it to the library immediately (same path as the
 * Library panel's own upload) — by the time it's sent as context, it's
 * already durable in Gaia Cloud, not a pending local blob riding along
 * with the message.
 */
export default function Composer({ onSend, busy, onDraftChange }) {
  const [text, setText] = useState('');
  const [attachments, setAttachments] = useState([]);
  const [attaching, setAttaching] = useState(false);
  const taRef = useRef(null);

  const grow = (el) => {
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  const submit = () => {
    const t = text.trim();
    if (!t || busy) return;
    onSend(t, attachments);
    setText('');
    setAttachments([]);
    if (taRef.current) taRef.current.style.height = 'auto';
  };

  const onKey = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  const attachFile = async () => {
    setAttaching(true);
    try {
      const uploaded = await libraryApi.pickAndUploadFile();
      if (uploaded) setAttachments((prev) => [...prev, uploaded]);
    } catch (_) {
      // Calm failure: the file simply isn't attached. No error banner for
      // a cancelled or failed picker — the composer stays exactly as it was.
    } finally {
      setAttaching(false);
    }
  };

  const removeAttachment = (id) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  };

  return (
    <div className="composer-wrap">
      {attachments.length > 0 && (
        <div className="composer-attachments">
          {attachments.map((a) => (
            <span key={a.id} className="attachment-chip">
              <Paperclip size={11} />
              {a.filename}
              <button
                className="attachment-chip-remove"
                onClick={() => removeAttachment(a.id)}
                aria-label={L.libraryDelete}
              >
                <X size={11} />
              </button>
            </span>
          ))}
        </div>
      )}
      <div className="composer">
        <button
          className="composer-attach"
          onClick={attachFile}
          disabled={attaching || busy}
          aria-label={L.libraryUpload}
          title={L.composerAttach}
        >
          <Paperclip size={16} />
        </button>
        <textarea
          ref={taRef}
          className="composer-input"
          placeholder={busy ? '…' : L.composerPlaceholder}
          value={text}
          onChange={(e) => {
            setText(e.target.value);
            grow(e.target);
            if (onDraftChange) onDraftChange(e.target.value.trim().length > 0);
          }}
          onKeyDown={onKey}
          rows={1}
        />
        <button
          className="composer-send"
          onClick={submit}
          disabled={!text.trim() || busy}
          aria-label={L.send}
        >
          <ArrowUp size={18} />
        </button>
      </div>
    </div>
  );
}
