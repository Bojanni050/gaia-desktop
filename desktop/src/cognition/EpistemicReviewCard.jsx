/**
 * EpistemicReviewCard — one derived statement awaiting the human's judgement.
 *
 * Deliberately bare: the statement, one plain question, and the answers it
 * actually takes. For a hypothesis or a candidate model that is "Klopt dit?"
 * with Ja or Nee; for a link between two things, "Klopt dit verband?".
 * Everything that made this feel like a control panel — status, scope, raw
 * evidence ids, "examine this", re-wording — is gone. The only thing that still
 * hides behind "Meer" is what a person might actually want to weigh: the
 * objection the card held back, and a plain note of whether anything supports
 * or contradicts the claim. "Meer" only appears when there is such a thing to
 * reveal.
 *
 * An open question is not a claim to judge but a question to answer, and its
 * `question_shape` names the control: 'yes_no' gives the Ja/Nee pair AS the
 * answer, 'choice' gives one button per option the server offered, and 'open'
 * gives a plain text field. The server sets the shape — it derives one for
 * questions written before it existed — so nothing here guesses; an unknown
 * shape, and a 'choice' without two usable options, fall back to the text
 * field, which can take any question's answer. Answering is not a verdict: it
 * records the answer, changes no status and mirrors nothing, and the question
 * then leaves the list. "Loslaten" stays beside it — the question itself can
 * still be let go.
 *
 * A macro (high-impact) statement still carries its friction, but as a SECOND
 * step: "Ja" does not confirm it directly. It opens the objection the card held
 * back, states plainly what confirming means, and only then offers Bevestig.
 * A macro statement with no counter-hypothesis cannot be confirmed at all — the
 * server asks for the same, and the card says so. A question carries no such
 * friction: there is nothing to establish.
 *
 * A rejected statement is a hard quarantine: no answers at all, only an explicit
 * "Reconsider" that requires a stated reason — the one path back, mirrored by
 * the server.
 */
import React, { useState } from 'react';
import { Check, RotateCcw, X } from 'lucide-react';
import { L } from '../lib/lexicon';

// The prompt follows the kind of statement: "Klopt dit?" fits a hypothesis or a
// candidate model, but not a link between two things.
const ASK_BY_KIND = {
  relationship: L.cognitionAskRelationship,
};

const QUESTION_SHAPES = ['yes_no', 'open', 'choice'];

// What the person clicked, in a form that does not depend on the language they
// read it in: the answer is recorded, not shown back as a label.
const YES = 'yes';
const NO = 'no';

/**
 * The control an open question needs, or null for every other kind of
 * statement. `question_shape` is the server's: it derives one for questions
 * that predate the field, so the card only has to be careful with a shape it
 * does not know, and with a 'choice' whose options cannot carry it.
 */
export function questionControl(item) {
  if (!item || item.kind !== 'open_question') return null;
  const options = Array.isArray(item.question_options)
    ? item.question_options.filter((o) => typeof o === 'string' && o.trim())
    : [];
  const shape = QUESTION_SHAPES.includes(item.question_shape) ? item.question_shape : 'open';
  if (shape === 'choice' && options.length >= 2) return { shape, options };
  return { shape: shape === 'choice' ? 'open' : shape, options: [] };
}

export default function EpistemicReviewCard({ item, busy, onReject, onConfirm, onReopen, onAnswer }) {
  const [more, setMore] = useState(false);
  const [confirming, setConfirming] = useState(false); // macro: the second step
  const [reopenReason, setReopenReason] = useState('');
  const [answer, setAnswer] = useState('');

  const control = questionControl(item);
  const ask = ASK_BY_KIND[item.kind] || L.cognitionAsk;
  const isRejected = item.status === 'rejected';
  const isMacro = item.scope !== 'micro';
  const hasCounter = Boolean(item.counter_hypothesis && item.counter_hypothesis.trim());
  const evidenceFor = Array.isArray(item.evidence_for) ? item.evidence_for : [];
  const evidenceAgainst = Array.isArray(item.evidence_against) ? item.evidence_against : [];
  const hasEvidence = evidenceFor.length > 0 || evidenceAgainst.length > 0;
  const canReveal = hasCounter || hasEvidence;

  const canConfirmMacro = !busy && hasCounter;
  const canReopen = !busy && reopenReason.trim().length > 0;
  const canAnswer = !busy && answer.trim().length > 0;

  const handleYes = () => {
    if (isMacro) {
      setConfirming(true); // face the objection first — see the doc comment
      return;
    }
    onConfirm(item, { rationale: undefined, statement: undefined });
  };

  const handleConfirmMacro = () => {
    if (!canConfirmMacro) return;
    onConfirm(item, { rationale: L.cognitionImplicationEstablish, statement: undefined });
  };

  const handleReopen = () => {
    if (!canReopen) return;
    onReopen(item, reopenReason.trim());
  };

  const handleAnswer = (value) => {
    if (busy || !onAnswer) return;
    onAnswer(item, value);
  };

  const handleAnswerText = () => {
    if (!canAnswer) return;
    handleAnswer(answer.trim());
  };

  return (
    <div className="cognition-item epistemic-card">
      <div className="cognition-statement">{item.statement}</div>

      {isRejected ? (
        <div className="cognition-reopen">
          <span className="cognition-reopen-hint">{L.cognitionReopenHint}</span>
          <textarea
            value={reopenReason}
            onChange={(e) => setReopenReason(e.target.value)}
            placeholder={L.cognitionReopenPlaceholder}
            rows={2}
          />
          {reopenReason.trim().length === 0 && (
            <span className="cognition-implication-warning">{L.cognitionReopenRequired}</span>
          )}
          <div className="cognition-choice">
            <button
              className="cognition-reopen-action"
              onClick={handleReopen}
              disabled={!canReopen}
              aria-label={L.cognitionReopen}
            >
              <RotateCcw size={14} /> {L.cognitionReopen}
            </button>
          </div>
        </div>
      ) : confirming ? (
        <div className="cognition-confirm-step">
          {hasCounter ? (
            <div className="cognition-counter-body">
              <span className="cognition-counter-label">{L.cognitionCounterHypothesis}</span>
              <p>{item.counter_hypothesis}</p>
              <span className="cognition-counter-note">{L.cognitionCounterHint}</span>
            </div>
          ) : (
            <p className="cognition-counter-missing">{L.cognitionCounterMissing}</p>
          )}
          <p className="cognition-sure">{L.cognitionMacroSure}</p>
          <div className="cognition-choice">
            <button
              className="cognition-yes"
              onClick={handleConfirmMacro}
              disabled={!canConfirmMacro}
              aria-label={L.cognitionConfirm}
            >
              <Check size={14} /> {L.cognitionConfirm}
            </button>
            <button onClick={() => setConfirming(false)}>{L.cognitionBack}</button>
          </div>
        </div>
      ) : (
        <>
          <p className="cognition-ask">{control ? L.cognitionAskAnswer : ask}</p>

          {control && control.shape === 'open' && (
            <textarea
              className="cognition-answer-input"
              value={answer}
              onChange={(e) => setAnswer(e.target.value)}
              placeholder={L.cognitionAnswerPlaceholder}
              rows={2}
              aria-label={L.cognitionAnswerPlaceholder}
            />
          )}

          <div className="cognition-choice">
            {control ? (
              <>
                {control.shape === 'yes_no' && (
                  <>
                    <button
                      className="cognition-option"
                      onClick={() => handleAnswer(YES)}
                      disabled={busy}
                      aria-label={L.cognitionYes}
                    >
                      {L.cognitionYes}
                    </button>
                    <button
                      className="cognition-option"
                      onClick={() => handleAnswer(NO)}
                      disabled={busy}
                      aria-label={L.cognitionNo}
                    >
                      {L.cognitionNo}
                    </button>
                  </>
                )}
                {control.shape === 'choice' &&
                  control.options.map((option) => (
                    <button
                      key={option}
                      className="cognition-option"
                      onClick={() => handleAnswer(option)}
                      disabled={busy}
                      aria-label={option}
                    >
                      {option}
                    </button>
                  ))}
                {control.shape === 'open' && (
                  <button
                    className="cognition-yes"
                    onClick={handleAnswerText}
                    disabled={!canAnswer}
                    aria-label={L.cognitionAnswerAction}
                  >
                    <Check size={14} /> {L.cognitionAnswerAction}
                  </button>
                )}
                <button
                  type="button"
                  className="cognition-letgo"
                  onClick={() => onReject(item)}
                  disabled={busy}
                  aria-label={L.cognitionReject}
                >
                  {L.cognitionReject}
                </button>
              </>
            ) : (
              <>
                <button className="cognition-yes" onClick={handleYes} disabled={busy} aria-label={L.cognitionYes}>
                  <Check size={14} /> {L.cognitionYes}
                </button>
                <button className="cognition-no" onClick={() => onReject(item)} disabled={busy} aria-label={L.cognitionNo}>
                  <X size={14} /> {L.cognitionNo}
                </button>
              </>
            )}
            {canReveal && (
              <button
                type="button"
                className="cognition-more-toggle"
                onClick={() => setMore((v) => !v)}
                aria-expanded={more}
              >
                {more ? L.cognitionLess : L.cognitionMore}
              </button>
            )}
          </div>

          {more && (
            <div className="cognition-more">
              {hasCounter && (
                <div className="cognition-counter-body">
                  <span className="cognition-counter-label">{L.cognitionCounterHypothesis}</span>
                  <p>{item.counter_hypothesis}</p>
                  <span className="cognition-counter-note">{L.cognitionCounterHint}</span>
                </div>
              )}
              {evidenceFor.length > 0 && (
                <p className="cognition-evidence-note">{L.cognitionEvidenceForNote}</p>
              )}
              {evidenceAgainst.length > 0 && (
                <p className="cognition-evidence-note">{L.cognitionEvidenceAgainstNote}</p>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
