import React, { useCallback, useRef, useState } from 'react';
import { ArrowUp, Loader2, Paperclip, Speech, Square, X } from 'lucide-react';
import { libraryApi } from '../server/api';
import { usePushToTalk } from '../state/usePushToTalk';
import VoiceControls from './VoiceControls';
import { L } from '../lib/lexicon';
import { parseSlashCommand, matchCommands, dispatchSlashCommand } from '../lib/slashCommands';

/**
 * Composer — the web's pill, ported: Enter sends, Shift+Enter is a newline,
 * focus gathers a soft accent ring.
 *
 * Attaching a file uploads it to the library immediately (same path as the
 * Library panel's own upload) — by the time it's sent as context, it's
 * already durable in Gaia Cloud, not a pending local blob riding along
 * with the message. Pasting works the same way: an image (or any file) on
 * the clipboard becomes a chip, and pasted text stays text.
 *
 * The mic button is push-to-talk: with the microphone off a click switches it on, then one click
 * starts a recording and the next ends it. What was said arrives as text, which is sent exactly like
 * typed text — unless a turn is already running or a draft is being typed, in which case it is put in
 * the text box instead, so nothing spoken is ever lost or sent over something half-written.
 *
 * While a turn runs the send arrow becomes a stop button: the same place, one meaning — where you
 * would say something, you can now cut her off. `onStop` is the caller's cancel (the desktop closes
 * the stream through Rust); without it the arrow simply stays disabled.
 *
 * Slash commands: typing `/` opens an autocomplete dropdown of available commands. Commands are
 * dispatched locally (never sent to Gaia Cloud) and the input is cleared. `onCommand` receives the
 * callbacks needed for action commands (new, stop, retry, undo).
 */
export default function Composer({ onSend, onStop, busy, onDraftChange, listenState = 'off', onCommand }) {
  const [text, setText] = useState('');
  const [attachments, setAttachments] = useState([]);
  const [attaching, setAttaching] = useState(false);
  const [pasteFailed, setPasteFailed] = useState(false);
  const [commandNote, setCommandNote] = useState(null);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashQuery, setSlashQuery] = useState('');
  const [slashIndex, setSlashIndex] = useState(0);
  const taRef = useRef(null);
  // The transcript handler needs the freshest draft/busy without re-subscribing to events.
  const latest = useRef({});
  latest.current = { text, busy, onSend, attachments };

  const grow = (el) => {
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 220)}px`;
  };

  // Slash command autocomplete: matches the text after `/` until a space.
  const slashMatches = slashOpen ? matchCommands(slashQuery) : [];
  const showSlashDropdown = slashOpen && slashMatches.length > 0;

  const submit = () => {
    const t = text.trim();
    if (!t || busy) return;

    // Slash command? Dispatch locally, never send to Gaia Cloud.
    const parsed = parseSlashCommand(t);
    if (parsed) {
      const result = dispatchSlashCommand(parsed, onCommand || {}, { busy });
      if (result.handled) {
        setText('');
        setAttachments([]);
        setSlashOpen(false);
        setSlashQuery('');
        if (taRef.current) taRef.current.style.height = 'auto';
        if (result.message) {
          setCommandNote(result.message);
          setTimeout(() => setCommandNote(null), 4000);
        }
        return;
      }
    }

    onSend(t, attachments);
    setText('');
    setAttachments([]);
    if (taRef.current) taRef.current.style.height = 'auto';
  };

  // A running turn turns the arrow into her stop button, so the one control
  // that sends is also the one that interrupts.
  const canStop = Boolean(busy && typeof onStop === 'function');

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
    // Autocomplete navigation: ArrowUp/Down cycles, Tab completes, Escape closes.
    if (showSlashDropdown) {
      if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSlashIndex((i) => (i + 1) % slashMatches.length);
        return;
      }
      if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSlashIndex((i) => (i - 1 + slashMatches.length) % slashMatches.length);
        return;
      }
      if (e.key === 'Tab') {
        e.preventDefault();
        const cmd = slashMatches[slashIndex];
        if (cmd) {
          setText(`/${cmd.name} `);
          setSlashOpen(false);
          setSlashQuery('');
          requestAnimationFrame(() => taRef.current && taRef.current.focus());
        }
        return;
      }
      if (e.key === 'Escape') {
        e.preventDefault();
        setSlashOpen(false);
        setSlashQuery('');
        return;
      }
    }
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      submit();
    }
  };

  const onTextChange = (value) => {
    setText(value);
    if (onDraftChange) onDraftChange(value.trim().length > 0);

    // Detect slash command typing: `/` at start, no space yet (or first word is a command).
    const trimmed = value.trimStart();
    if (trimmed.startsWith('/')) {
      const afterSlash = trimmed.slice(1);
      const spaceIndex = afterSlash.indexOf(' ');
      if (spaceIndex === -1) {
        // Still typing the command name — show autocomplete.
        setSlashOpen(true);
        setSlashQuery(afterSlash.toLowerCase());
        setSlashIndex(0);
      } else {
        // Has arguments — close autocomplete.
        setSlashOpen(false);
        setSlashQuery('');
      }
    } else {
      setSlashOpen(false);
      setSlashQuery('');
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

  /**
   * Files on the clipboard go to the library exactly like the paperclip's own
   * pick — the bytes never touch this device's disk. Each one is uploaded on
   * its own; whatever made it becomes a chip even if another did not.
   */
  const attachPasted = async (files) => {
    setPasteFailed(false);
    setAttaching(true);
    const uploaded = [];
    try {
      for (const file of files) {
        const bytes = new Uint8Array(await file.arrayBuffer());
        uploaded.push(await libraryApi.uploadPastedFile(bytes, pastedFileName(file)));
      }
    } catch (_) {
      setPasteFailed(true);
    } finally {
      if (uploaded.length > 0) setAttachments((prev) => [...prev, ...uploaded]);
      setAttaching(false);
    }
  };

  const onPaste = (e) => {
    const files = clipboardFiles(e.clipboardData);
    // Pasted text is text: only a file is taken out of the clipboard.
    if (files.length === 0) return;
    e.preventDefault();
    attachPasted(files);
  };

  const micLabel = !ptt.micOn
    ? L.micSwitchOn
    : ptt.phase === 'recording'
      ? L.micStop
      : ptt.phase === 'transcribing'
        ? L.micTranscribing
        : L.micTalk;

  // Click a command in the dropdown: fill it in and focus.
  const pickCommand = (cmd) => {
    setText(`/${cmd.name} `);
    setSlashOpen(false);
    setSlashQuery('');
    requestAnimationFrame(() => taRef.current && taRef.current.focus());
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

      {/* Slash command autocomplete dropdown */}
      {showSlashDropdown && (
        <div className="slash-dropdown" role="listbox">
          {slashMatches.map((cmd, i) => (
            <button
              key={cmd.name}
              className={`slash-item${i === slashIndex ? ' is-active' : ''}`}
              role="option"
              aria-selected={i === slashIndex}
              onClick={() => pickCommand(cmd)}
              onMouseEnter={() => setSlashIndex(i)}
              type="button"
            >
              <span className="slash-item-name">
                /{cmd.name}
                {cmd.aliases.length > 0 && (
                  <span className="slash-item-aliases">
                    {cmd.aliases.map((a) => `, /${a}`).join('')}
                  </span>
                )}
              </span>
              <span className="slash-item-hint">{cmd.hint()}</span>
            </button>
          ))}
          <div className="slash-dropdown-footer">{L.slashAutocompleteHint}</div>
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
            ) : (
              <Speech size={16} />
            )}
          </button>
        )}
        <VoiceControls listenState={listenState} />
        <textarea
          ref={taRef}
          className="composer-input"
          placeholder={busy ? '…' : ptt.phase === 'recording' ? L.micListeningPlaceholder : L.composerPlaceholder}
          value={text}
          onChange={(e) => {
            onTextChange(e.target.value);
            grow(e.target);
          }}
          onKeyDown={onKey}
          onPaste={onPaste}
          rows={1}
        />
        {canStop ? (
          <button
            className="composer-send is-stop"
            onClick={() => onStop()}
            aria-label={L.stop}
            title={L.stop}
            type="button"
          >
            <Square size={14} fill="currentColor" />
          </button>
        ) : (
          <button
            className="composer-send"
            onClick={submit}
            disabled={!text.trim() || busy}
            aria-label={L.send}
          >
            <ArrowUp size={18} />
          </button>
        )}
      </div>
      {commandNote && (
        <p className="composer-note slash-note" role="status">
          {commandNote}
        </p>
      )}
      {(ptt.phase === 'noSpeech' || ptt.phase === 'failed') && (
        <p className="composer-note" role="status">
          {ptt.phase === 'noSpeech' ? L.micNoSpeech : micErrorText(ptt.error)}
        </p>
      )}
      {pasteFailed && (
        <p className="composer-note" role="status">
          {L.libraryUploadFailed}
        </p>
      )}
    </div>
  );
}

/** The files on the clipboard: a screenshot is one, copied text is none. */
function clipboardFiles(data) {
  if (!data) return [];
  const items = Array.from(data.items || []).filter((item) => item.kind === 'file');
  const files = items.length > 0 ? items.map((item) => item.getAsFile()) : Array.from(data.files || []);
  return files.filter(Boolean);
}

/**
 * What to call a pasted file. A file copied in Explorer still knows its name;
 * an image off the clipboard does not, so it gets one built from its type and
 * the moment it was pasted. Extension matters beyond looks: it is how Rust
 * labels the upload's MIME type.
 */
function pastedFileName(file) {
  const name = String(file.name || '').trim();
  if (name && name !== 'image.png') return name;
  const now = new Date();
  const pad = (n) => String(n).padStart(2, '0');
  const stamp = `${now.getFullYear()}${pad(now.getMonth() + 1)}${pad(now.getDate())}-${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `pasted-image-${stamp}.${extensionFor(file.type)}`;
}

function extensionFor(mimeType) {
  return (
    {
      'image/png': 'png',
      'image/jpeg': 'jpg',
      'image/gif': 'gif',
      'image/webp': 'webp',
      'image/bmp': 'bmp',
      'image/svg+xml': 'svg',
      'application/pdf': 'pdf',
    }[mimeType] || 'bin'
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
