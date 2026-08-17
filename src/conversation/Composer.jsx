import React, { useRef, useState } from 'react';
import { ArrowUp } from 'lucide-react';
import { L } from '../lib/lexicon';

/**
 * Composer — the web's pill, ported: Enter sends, Shift+Enter is a newline,
 * focus gathers a soft accent ring.
 */
export default function Composer({ onSend, busy, onDraftChange }) {
  const [text, setText] = useState('');
  const taRef = useRef(null);

  const grow = (el) => {
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  const submit = () => {
    const t = text.trim();
    if (!t || busy) return;
    onSend(t);
    setText('');
    if (taRef.current) taRef.current.style.height = 'auto';
  };

  const onKey = (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  return (
    <div className="composer-wrap">
      <div className="composer">
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
          aria-label="Send"
        >
          <ArrowUp size={18} />
        </button>
      </div>
    </div>
  );
}
