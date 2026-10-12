import React, { useState } from 'react';
import { parseResearchApproval } from '../state/contract';

/**
 * ResearchStatus — what is happening in the background, said plainly.
 *
 * Two things live here, and they are deliberately different in weight:
 *
 *   - Work in progress is a quiet line. It is not a conversation turn and must
 *     not read like one; it only answers "is she still on it?".
 *   - A run that parked on a command is a QUESTION, and it gets the weight of
 *     one: the command itself, what Hermes says it does, and the decisions the
 *     server will actually accept. Nothing is approved on the person's behalf,
 *     and the buttons come from the job's own `choices` rather than a list this
 *     component invented.
 *
 * Findings are not rendered here: the server writes them into the conversation,
 * so they appear as a normal message (the caller re-reads the thread).
 */

const CHOICE_LABELS = Object.freeze({
  once: 'Eenmalig',
  session: 'Deze sessie',
  always: 'Altijd',
  deny: 'Weigeren',
});

export default function ResearchStatus({
  running = [],
  awaiting = [],
  finished = [],
  activeConversationId,
  onAnswer,
  onOpen,
}) {
  const [answering, setAnswering] = useState(null);
  const [error, setError] = useState(null);

  // Finished work for a conversation that is not on screen: worth saying, not
  // worth hijacking the view.
  const elsewhere = finished.filter(
    (job) => job.conversationId && job.conversationId !== activeConversationId
  );

  if (running.length === 0 && awaiting.length === 0 && elsewhere.length === 0) return null;

  const answer = async (job, choice) => {
    setAnswering(job.id);
    setError(null);
    try {
      await onAnswer(job.id, choice);
    } catch (err) {
      setError((err && err.message) || 'De beslissing kon niet worden doorgegeven.');
    } finally {
      setAnswering(null);
    }
  };

  return (
    <div className="research-status">
      {awaiting.map((job) => {
        const approval = parseResearchApproval(job);
        if (!approval) return null;
        return (
          <div className="research-question" role="alert" key={job.id}>
            <p className="research-question-title">
              Hermes wil voor dit onderzoek een commando uitvoeren en vraagt om goedkeuring.
            </p>
            {approval.description && (
              <p className="research-question-note">{approval.description}</p>
            )}
            <pre className="research-command">{approval.command}</pre>
            <div className="research-choices">
              {approval.choices.map((choice) => (
                <button
                  key={choice}
                  type="button"
                  className={`research-choice${choice === 'deny' ? ' research-choice-deny' : ''}`}
                  disabled={answering === job.id}
                  onClick={() => answer(job, choice)}
                >
                  {CHOICE_LABELS[choice] || choice}
                </button>
              ))}
            </div>
            {error && answering === null && <p className="research-error">{error}</p>}
          </div>
        );
      })}

      {running.length > 0 && (
        <div className="research-running" role="status">
          <span className="research-dot" aria-hidden="true" />
          {running.length === 1
            ? 'Onderzoek loopt bij Hermes — je krijgt het resultaat zodra het er is.'
            : `${running.length} onderzoeken lopen bij Hermes — je krijgt de resultaten zodra ze er zijn.`}
        </div>
      )}

      {elsewhere.map((job) => (
        <div className="research-elsewhere" key={job.id}>
          <span>Onderzoek is klaar in een ander gesprek.</span>
          <button type="button" className="research-open" onClick={() => onOpen(job)}>
            Openen
          </button>
        </div>
      ))}
    </div>
  );
}
