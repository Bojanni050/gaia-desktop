/**
 * EpistemicReviewCard — one derived statement awaiting the human's judgement.
 *
 * Deliberately bare: the statement, one plain question, two answers (Ja or
 * Nee). The question follows the kind of statement — "Klopt dit?" for a
 * hypothesis, "Klopt dit verband?" for a relationship, "Is dit nog open?" for an
 * open question. Everything that made this feel like a control panel — status,
 * scope, raw evidence ids, "examine this", re-wording — is gone. The only thing
 * that still hides behind "Meer" is what a person might actually want to weigh:
 * the objection the card held back, and a plain note of whether anything
 * supports or contradicts the claim. "Meer" only appears when there is such a
 * thing to reveal.
 *
 * A macro (high-impact) statement still carries its friction, but as a SECOND
 * step: "Ja" does not confirm it directly. It opens the objection the card held
 * back, states plainly what confirming means, and only then offers Bevestig.
 * A macro statement with no counter-hypothesis cannot be confirmed at all — the
 * server asks for the same, and the card says so.
 *
 * A rejected statement is a hard quarantine: no Ja/Nee at all, only an explicit
 * "Reconsider" that requires a stated reason — the one path back, mirrored by
 * the server.
 */
import React, { useState } from 'react';
import { Check, RotateCcw, X } from 'lucide-react';
import { L } from '../lib/lexicon';

// The prompt follows the kind of statement: "Klopt dit?" fits a hypothesis or a
// candidate model, but not a link between two things, and certainly not a
// statement that is already phrased as an open question.
const ASK_BY_KIND = {
  relationship: L.cognitionAskRelationship,
  open_question: L.cognitionAskOpenQuestion,
};

export default function EpistemicReviewCard({ item, busy, onReject, onConfirm, onReopen }) {
  const [more, setMore] = useState(false);
  const [confirming, setConfirming] = useState(false); // macro: the second step
  const [reopenReason, setReopenReason] = useState('');

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
          <p className="cognition-ask">{ask}</p>
          <div className="cognition-choice">
            <button className="cognition-yes" onClick={handleYes} disabled={busy} aria-label={L.cognitionYes}>
              <Check size={14} /> {L.cognitionYes}
            </button>
            <button className="cognition-no" onClick={() => onReject(item)} disabled={busy} aria-label={L.cognitionNo}>
              <X size={14} /> {L.cognitionNo}
            </button>
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
