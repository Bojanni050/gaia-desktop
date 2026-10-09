/**
 * EpistemicReviewCard — one derived statement awaiting the human's judgement.
 *
 * Kept deliberately simple for someone meeting it for the first time: one plain
 * question, "Klopt dit?", and two answers, Ja or Nee. Everything that made this
 * card feel like a control panel — status, scope, evidence, the quarantined
 * counter-hypothesis, re-wording, "examine this" — lives behind a single "Meer"
 * disclosure, opened only when someone actually wants it.
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
import { Check, Lightbulb, RotateCcw, X } from 'lucide-react';
import { L } from '../lib/lexicon';

const STATUS_LABEL = {
  proposed: () => L.cognitionStatusProposed,
  testing: () => L.cognitionStatusTesting,
  corroborated: () => L.cognitionStatusCorroborated,
  confirmed: () => L.cognitionStatusConfirmed,
  rejected: () => L.cognitionStatusRejected,
};

export default function EpistemicReviewCard({ item, busy, onTest, onReject, onConfirm, onReopen }) {
  const [more, setMore] = useState(false);
  const [confirming, setConfirming] = useState(false); // macro: the second step
  const [nuancing, setNuancing] = useState(false);
  const [statementDraft, setStatementDraft] = useState(item.statement || '');
  const [reopenReason, setReopenReason] = useState('');

  const isRejected = item.status === 'rejected';
  const isMacro = item.scope !== 'micro';
  const hasCounter = Boolean(item.counter_hypothesis && item.counter_hypothesis.trim());
  const evidenceFor = Array.isArray(item.evidence_for) ? item.evidence_for : [];
  const evidenceAgainst = Array.isArray(item.evidence_against) ? item.evidence_against : [];
  const hasEvidence = evidenceFor.length > 0 || evidenceAgainst.length > 0;

  const canConfirmMacro = !busy && hasCounter;
  const canReopen = !busy && reopenReason.trim().length > 0;

  // The human's own re-wording, when they used "Nuanceren" and changed it.
  const nuancedStatement = () => {
    const normalized = statementDraft.trim();
    return nuancing && normalized && normalized !== item.statement ? normalized : undefined;
  };

  const handleYes = () => {
    if (isMacro) {
      setConfirming(true); // face the objection first — see the doc comment
      return;
    }
    onConfirm(item, { rationale: undefined, statement: nuancedStatement() });
  };

  const handleConfirmMacro = () => {
    if (!canConfirmMacro) return;
    onConfirm(item, { rationale: L.cognitionImplicationEstablish, statement: nuancedStatement() });
  };

  const handleReopen = () => {
    if (!canReopen) return;
    onReopen(item, reopenReason.trim());
  };

  const statusLabel = (STATUS_LABEL[item.status] || (() => item.status))();

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
          <p className="cognition-ask">{L.cognitionAsk}</p>
          <div className="cognition-choice">
            <button className="cognition-yes" onClick={handleYes} disabled={busy} aria-label={L.cognitionYes}>
              <Check size={14} /> {L.cognitionYes}
            </button>
            <button className="cognition-no" onClick={() => onReject(item)} disabled={busy} aria-label={L.cognitionNo}>
              <X size={14} /> {L.cognitionNo}
            </button>
            <button
              type="button"
              className="cognition-more-toggle"
              onClick={() => setMore((v) => !v)}
              aria-expanded={more}
            >
              {more ? L.cognitionLess : L.cognitionMore}
            </button>
          </div>

          {more && (
            <div className="cognition-more">
              <div className="cognition-meta">
                <span className="cognition-status">{statusLabel}</span>
                <span className={`cognition-scope cognition-scope-${isMacro ? 'macro' : 'micro'}`}>
                  {isMacro ? L.cognitionScopeMacro : L.cognitionScopeMicro}
                </span>
              </div>

              <div className="cognition-evidence">
                {!hasEvidence && <span className="cognition-evidence-empty">{L.cognitionNoEvidence}</span>}
                {evidenceFor.length > 0 && (
                  <span className="cognition-evidence-group">
                    <em>{L.cognitionEvidenceFor}</em>
                    {evidenceFor.map((id) => <code key={id}>{id}</code>)}
                  </span>
                )}
                {evidenceAgainst.length > 0 && (
                  <span className="cognition-evidence-group">
                    <em>{L.cognitionEvidenceAgainst}</em>
                    {evidenceAgainst.map((id) => <code key={id}>{id}</code>)}
                  </span>
                )}
              </div>

              {hasCounter && (
                <div className="cognition-counter-body">
                  <span className="cognition-counter-label">{L.cognitionCounterHypothesis}</span>
                  <p>{item.counter_hypothesis}</p>
                  <span className="cognition-counter-note">{L.cognitionCounterHint}</span>
                </div>
              )}

              <div className="cognition-choice">
                {item.status !== 'testing' && (
                  <button onClick={() => onTest(item)} disabled={busy} aria-label={L.cognitionTesting}>
                    <Lightbulb size={14} /> {L.cognitionTesting}
                  </button>
                )}
                <button
                  type="button"
                  onClick={() => setNuancing((v) => !v)}
                  disabled={busy}
                  aria-pressed={nuancing}
                  aria-label={L.cognitionNuance}
                >
                  {L.cognitionNuance}
                </button>
              </div>

              {nuancing && (
                <div className="cognition-nuance">
                  <span className="cognition-nuance-hint">{L.cognitionNuanceHint}</span>
                  <textarea
                    value={statementDraft}
                    onChange={(e) => setStatementDraft(e.target.value)}
                    placeholder={L.cognitionNuancePlaceholder}
                    rows={2}
                  />
                </div>
              )}
            </div>
          )}
        </>
      )}
    </div>
  );
}
