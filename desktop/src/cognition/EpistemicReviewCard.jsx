/**
 * EpistemicReviewCard — one derived statement awaiting the human's judgement.
 *
 * The human Absolute Override, but with structural friction (V3 review): a
 * macro statement is never a single yes/no click. To confirm one the person
 * must first open the quarantined counter-hypothesis AND answer an implication
 * test correctly; only then does Confirm enable. Micro statements stay a
 * single calm act — they are low-impact and may soft-promote on their own.
 *
 * The card renders only what the record actually carries (statement, evidence
 * ids, counter_hypothesis, scope). It never invents an implication specific to
 * the statement — the multiple-choice test below is a fixed, honest stand-in
 * until Logos can generate statement-specific implications.
 */
import React, { useState } from 'react';
import { Check, ChevronRight, Lightbulb, X } from 'lucide-react';
import { L } from '../lib/lexicon';

const STATUS_LABEL = {
  proposed: () => L.cognitionStatusProposed,
  testing: () => L.cognitionStatusTesting,
  corroborated: () => L.cognitionStatusCorroborated,
  confirmed: () => L.cognitionStatusConfirmed,
  rejected: () => L.cognitionStatusRejected,
};

const IMPLICATIONS = ['establish', 'counter', 'soft'];

export default function EpistemicReviewCard({ item, busy, onTest, onReject, onConfirm }) {
  const [showEvidence, setShowEvidence] = useState(false);
  const [counterOpen, setCounterOpen] = useState(false);
  const [nuancing, setNuancing] = useState(false);
  const [statementDraft, setStatementDraft] = useState(item.statement || '');
  const [implication, setImplication] = useState(null);

  const isMacro = item.scope !== 'micro';
  const hasCounter = Boolean(item.counter_hypothesis && item.counter_hypothesis.trim());
  const evidenceFor = Array.isArray(item.evidence_for) ? item.evidence_for : [];
  const evidenceAgainst = Array.isArray(item.evidence_against) ? item.evidence_against : [];
  const hasEvidence = evidenceFor.length > 0 || evidenceAgainst.length > 0;

  // Confirm unlocks only when nothing is in flight AND, for a macro statement,
  // the person has looked at the objection and answered the implication test.
  const canConfirm = !busy && (!isMacro || (counterOpen && implication === 'establish'));

  const implicationText = (key) => ({
    establish: L.cognitionImplicationEstablish,
    counter: L.cognitionImplicationCounter,
    soft: L.cognitionImplicationSoft,
  }[key]);

  const handleConfirm = (e) => {
    e.stopPropagation();
    if (!canConfirm) return;
    const normalized = statementDraft.trim();
    const statement = nuancing && normalized && normalized !== item.statement ? normalized : undefined;
    // A macro confirmation always carries a rationale (server-side friction
    // too): the implication the person consciously chose.
    const rationale = isMacro ? implicationText(implication) : undefined;
    onConfirm(item, { rationale, statement });
  };

  const handleReject = (e) => { e.stopPropagation(); onReject(item); };
  const handleTest = (e) => { e.stopPropagation(); onTest(item); };

  const statusLabel = (STATUS_LABEL[item.status] || (() => item.status))();

  return (
    <div className="cognition-item epistemic-card">
      <div className="cognition-statement">{item.statement}</div>

      <div className="cognition-meta">
        <span className="cognition-status">{statusLabel}</span>
        <span className={`cognition-scope cognition-scope-${isMacro ? 'macro' : 'micro'}`}>
          {isMacro ? L.cognitionScopeMacro : L.cognitionScopeMicro}
        </span>
      </div>

      <button
        type="button"
        className="cognition-toggle"
        aria-expanded={showEvidence}
        onClick={(e) => { e.stopPropagation(); setShowEvidence((v) => !v); }}
      >
        <ChevronRight size={12} className={`cognition-chevron${showEvidence ? ' open' : ''}`} />
        {L.cognitionEvidence}
      </button>
      {showEvidence && (
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
      )}

      {hasCounter ? (
        <div className="cognition-counter">
          <button
            type="button"
            className="cognition-toggle"
            aria-expanded={counterOpen}
            onClick={(e) => { e.stopPropagation(); setCounterOpen((v) => !v); }}
          >
            <ChevronRight size={12} className={`cognition-chevron${counterOpen ? ' open' : ''}`} />
            {counterOpen ? L.cognitionHideCounter : L.cognitionShowCounter}
          </button>
          {counterOpen && (
            <div className="cognition-counter-body">
              <span className="cognition-counter-label">{L.cognitionCounterHypothesis}</span>
              <p>{item.counter_hypothesis}</p>
              <span className="cognition-counter-note">{L.cognitionCounterHint}</span>
            </div>
          )}
        </div>
      ) : (
        <p className="cognition-counter-missing">{L.cognitionCounterMissing}</p>
      )}

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

      {isMacro && (
        <div className="cognition-implication">
          <span className="cognition-implication-prompt">{L.cognitionImplicationPrompt}</span>
          {IMPLICATIONS.map((key) => (
            <label key={key} className="cognition-implication-option">
              <input
                type="radio"
                name={`implication-${item.id}`}
                checked={implication === key}
                onChange={() => setImplication(key)}
                onClick={(e) => e.stopPropagation()}
              />
              {implicationText(key)}
            </label>
          ))}
          {implication && implication !== 'establish' && (
            <span className="cognition-implication-warning">{L.cognitionImplicationPickCorrect}</span>
          )}
        </div>
      )}

      <div className="thread-actions">
        {item.status !== 'testing' && (
          <button
            className="cognition-action"
            onClick={handleTest}
            disabled={busy}
            aria-label={L.cognitionTesting}
            title={L.cognitionTesting}
          >
            <Lightbulb size={14} />
          </button>
        )}
        <button
          type="button"
          className="cognition-action cognition-nuance-toggle"
          onClick={(e) => { e.stopPropagation(); setNuancing((v) => !v); }}
          disabled={busy}
          aria-pressed={nuancing}
          title={L.cognitionNuance}
        >
          {L.cognitionNuance}
        </button>
        <button
          className="cognition-action cognition-confirm"
          onClick={handleConfirm}
          disabled={!canConfirm}
          aria-label={L.cognitionConfirm}
          title={L.cognitionConfirm}
        >
          <Check size={14} />
        </button>
        <button
          className="cognition-action cognition-reject"
          onClick={handleReject}
          disabled={busy}
          aria-label={L.cognitionReject}
          title={L.cognitionReject}
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}
