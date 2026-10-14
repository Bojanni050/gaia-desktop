import React from 'react';
import { L } from '../lib/lexicon';

/**
 * DesktopPrompt — Gaia asks before she does something on this computer.
 *
 * The person must be able to see exactly what will happen, so the server's one-line
 * summary is shown together with the whole input: the full command, or the path and
 * the full content of the file. Nothing runs until "Toestaan" is pressed; a prompt
 * that is ignored simply expires on the server.
 */
function InputDetail({ skill, input }) {
  if (skill === 'run-command') {
    return <pre className="research-command">{String(input.command ?? '')}</pre>;
  }
  if (skill === 'file-write') {
    return (
      <>
        <pre className="research-command">{String(input.path ?? '')}</pre>
        <pre className="research-command desktop-content">{String(input.content ?? '')}</pre>
      </>
    );
  }
  return <pre className="research-command">{JSON.stringify(input, null, 2)}</pre>;
}

export default function DesktopPrompt({ prompts = [], notice, onDecide, onDismissNotice }) {
  if (prompts.length === 0 && !notice) return null;
  return (
    <div className="research-status desktop-status">
      {prompts.map((action) => (
        <div className="research-question" role="alert" key={action.id}>
          <p className="research-question-title">{L.desktopAsk}</p>
          <p className="research-question-note">{action.summary}</p>
          <InputDetail skill={action.skill} input={action.input} />
          <div className="research-choices">
            <button type="button" className="research-choice" onClick={() => onDecide(action.id, true)}>
              {L.desktopAllow}
            </button>
            <button
              type="button"
              className="research-choice research-choice-deny"
              onClick={() => onDecide(action.id, false)}
            >
              {L.desktopDeny}
            </button>
          </div>
        </div>
      ))}
      {notice && (
        <div className="research-elsewhere" role="status">
          <span>{notice}</span>
          <button type="button" className="research-open" onClick={onDismissNotice}>
            {L.desktopNoticeClose}
          </button>
        </div>
      )}
    </div>
  );
}
