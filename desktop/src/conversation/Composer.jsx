import React, { useCallback, useRef, useState } from 'react';
import { ArrowUp, Loader2, Mic, MicOff, Paperclip, X } from 'lucide-react';
import { libraryApi } from '../server/api';
import { usePushToTalk } from '../state/usePushToTalk';
import { L } from '../lib/lexicon';

/**
 * Composer — the web's pill, ported: Enter sends, Shift+Enter is a newline,
 * focus gathers a soft accent ring.
 *
 * Attaching a file uploads it to the library immediately (same path as the
 * Library panel's own upload) — by the time it's sent as context, it's
 * already durable in Gaia Cloud, not a pending local blob riding along
 * with the message.
 *
 * The mic button is push-to-talk: with the microphone off a click switches it on, then one click
 * starts a recording and the next ends it. What was said arrives as text, which is sent exactly like
 * typed text — unless a turn is already running or a draft is being typed, in which case it is put in
 * the text box instead, so nothing spoken is ever lost or sent over something half-written.
 */
export default function Composer({ onSend, busy, onDraftChange }) {
  const [text, setText] = useState('');
  const [attachments, setAttachments] = useState([]);
  const [attaching, setAttaching] = useState(false);
  const taRef = useRef(null);
  // The transcript handler needs the freshest draft/busy without re-subscribing to events.
  const latest = useRef({});
  latest.current = { text, busy, onSend, attachments };

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

  const onTranscript = useCallback((spoken) => {
    const said = String(spoken || '').trim();
    if (!said) return;
    const { text: draft, busy: running, onSend: send, attachments: files } = latest.current;
    if (!running && !draft.trim()) {
      send(said, files);
      setAttachments([]);
      return;
    }
    // Do not send over a running turn or a half-written message: add it to the draft.
    setText((prev) => (prev.trim() ? `${prev.trim()} ${said}` : said));
    if (onDraftChange) onDraftChange(true);
    requestAnimationFrame(() => taRef.current && grow(taRef.current));
  }, [onDraftChange]);

  const ptt = usePushToTalk({ onTranscript });

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

  const micLabel = !ptt.micOn
    ? L.micSwitchOn
    : ptt.phase === 'recording'
      ? L.micStop
      : ptt.phase === 'transcribing'
        ? L.micTranscribing
        : L.micTalk;

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
        {ptt.available && (
          <button
            className={`composer-mic${ptt.phase === 'recording' ? ' is-recording' : ''}${ptt.micOn ? '' : ' is-off'}`}
            onClick={ptt.toggle}
            disabled={ptt.phase === 'transcribing'}
            aria-label={micLabel}
            aria-pressed={ptt.phase === 'recording'}
            title={micLabel}
            type="button"
          >
            {ptt.phase === 'transcribing' ? (
              <Loader2 size={16} className="spin" />
            ) : ptt.micOn ? (
              <Mic size={16} />
            ) : (
              <MicOff size={16} />
            )}
          </button>
        )}
        <textarea
          ref={taRef}
          className="composer-input"
          placeholder={busy ? '…' : ptt.phase === 'recording' ? L.micListeningPlaceholder : L.composerPlaceholder}
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
      {(ptt.phase === 'noSpeech' || ptt.phase === 'failed') && (
        <p className="composer-mic-note" role="status">
          {ptt.phase === 'noSpeech' ? L.micNoSpeech : micErrorText(ptt.error)}
        </p>
      )}
    </div>
  );
}

/** One calm sentence for why a recording did not become text. */
function micErrorText(error) {
  const code = error && error.code;
  return (
    {
      mic_off: L.micErrMicOff,
      mic_interrupted: L.micErrInterrupted,
      busy: L.micErrBusy,
      unauthorized: L.micErrUnauthorized,
      not_configured: L.micErrNotConfigured,
      not_supported: L.micErrNotConfigured,
      too_long: L.micErrTooLong,
      unreachable: L.micErrUnreachable,
    }[code] || L.micErrGeneric
  );
}
