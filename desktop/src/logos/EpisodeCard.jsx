/**
 * EpisodeCard — one stretch of activity Logos recognised, as a card.
 *
 * The whole point of the card is an epistemic separation the eye can read in
 * one glance: the interpretation is the loud, human sentence at the top; the
 * observations capture-rs actually recorded are quiet, collapsed, and only
 * appear on request. Nothing here is allowed to blur the two — the badge says
 * "Episode / Interpretation", the evidence section says "raw observations" and
 * carries a note that Logos did not write it.
 *
 * Pure display: the card owns only which evidence is open. The two actions are
 * callbacks; what "Ask Gaia" and "Save as pattern" mean is decided elsewhere.
 */
import React, { useState } from 'react';
import {
  Bookmark,
  ChevronDown,
  Clock,
  FileSpreadsheet,
  FileText,
  Globe,
  Mail,
  MessageCircle,
  Monitor,
  ScanText,
  ScrollText,
  Sparkles,
  Terminal,
} from 'lucide-react';
import { L } from '../lib/lexicon';

const pad = (n) => String(n).padStart(2, '0');

/** "10:15" from an ISO string, parsed as wall-clock (no locale surprises). */
function clock(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** "10:15:12" for the observation rows. */
function clockSeconds(iso) {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return `${clock(iso)}:${pad(d.getSeconds())}`;
}

/**
 * The app is the strongest hint (Outlook is mail whichever way you name it),
 * the declared `type` is the fallback. Unknown stays neutral — never guessed
 * into the wrong icon.
 */
function contextIcon({ application = '', type }) {
  const app = application.toLowerCase();
  if (/excel|sheets|numbers|calc/.test(app)) return FileSpreadsheet;
  if (/word|docs|pages|writer/.test(app)) return FileText;
  if (/outlook|mail|thunderbird|teams/.test(app)) return Mail;
  if (/chrome|edge|firefox|safari|browser|brave|arc/.test(app)) return Globe;
  if (/terminal|powershell|cmd|bash|zsh|console/.test(app)) return Terminal;
  if (type === 'email') return Mail;
  if (type === 'document' || type === 'file') return FileText;
  if (type === 'webpage') return Globe;
  return Monitor;
}

export default function EpisodeCard({ episode, onAskGaia, onSavePattern }) {
  const [evidenceOpen, setEvidenceOpen] = useState(false);

  const context = Array.isArray(episode.context) ? episode.context : [];
  const observations = Array.isArray(episode.observations) ? episode.observations : [];
  const panelId = `episode-evidence-${episode.id}`;
  const confidence =
    typeof episode.confidence === 'number' ? Math.round(episode.confidence * 100) : null;

  return (
    <article className="episode-card">
      <header className="episode-head">
        <div className="episode-head-main">
          <div className="episode-meta">
            <span className="episode-time">
              <Clock size={12} aria-hidden="true" />
              {clock(episode.startTime)} – {clock(episode.endTime)}
            </span>
            {episode.session && (
              <span className="episode-session">
                {L.logosSessionPrefix}: {episode.session}
              </span>
            )}
            {confidence !== null && (
              <span
                className="episode-confidence"
                role="img"
                aria-label={`${L.logosConfidence}: ${confidence}%`}
                title={`${L.logosConfidence}: ${confidence}%`}
              >
                <span className="episode-confidence-bars" aria-hidden="true">
                  {[0, 1, 2].map((i) => (
                    <i key={i} className={confidence >= (i + 1) * 33 ? 'on' : ''} />
                  ))}
                </span>
                <span aria-hidden="true">{L.logosConfidence}</span>
              </span>
            )}
          </div>
          <h3 className="episode-title">{episode.title}</h3>
        </div>

        <span className="episode-badge" title={L.logosBadgeHint}>
          <Sparkles size={12} aria-hidden="true" />
          {L.logosEpisodeBadge}
        </span>
      </header>

      <p className="episode-interpretation">{episode.interpretation}</p>

      {context.length > 0 && (
        <div className="episode-context">
          <span className="episode-context-label">{L.logosContext}</span>
          <div className="episode-chips">
            {context.map((item, index) => {
              const Icon = contextIcon(item);
              return (
                <span
                  key={`${item.application}-${index}`}
                  className="episode-chip"
                  title={item.object ? `${item.application}: ${item.object}` : item.application}
                >
                  <Icon size={13} className="episode-chip-icon" aria-hidden="true" />
                  <span className="episode-chip-app">{item.application}</span>
                  {item.object && (
                    <>
                      <span className="episode-chip-sep" aria-hidden="true">
                        ·
                      </span>
                      <span className="episode-chip-object">“{item.object}”</span>
                    </>
                  )}
                </span>
              );
            })}
          </div>
        </div>
      )}

      <button
        type="button"
        className="episode-evidence-toggle"
        aria-expanded={evidenceOpen}
        aria-controls={panelId}
        onClick={() => setEvidenceOpen((open) => !open)}
      >
        <ScrollText size={14} aria-hidden="true" />
        <span>{L.logosEvidenceToggle}</span>
        <span className="episode-evidence-count">
          ({observations.length} {observations.length === 1 ? L.logosCapture : L.logosCaptures})
        </span>
        <ChevronDown
          size={15}
          className={`episode-chevron${evidenceOpen ? ' open' : ''}`}
          aria-hidden="true"
        />
      </button>

      {evidenceOpen && (
        <section
          id={panelId}
          className="episode-evidence"
          role="region"
          aria-label={L.logosRawObservations}
        >
          <div className="episode-raw-head">
            <span className="episode-raw-title">{L.logosRawObservations}</span>
            <span className="episode-raw-note">{L.logosRawNote}</span>
          </div>

          <ol className="episode-observations">
            {observations.map((obs) => (
              <li key={obs.id} className="episode-observation">
                {obs.screenshot ? (
                  <img className="episode-shot" src={obs.screenshot} alt="" aria-hidden="true" />
                ) : (
                  <span className="episode-shot episode-shot-empty" aria-hidden="true">
                    <ScanText size={15} />
                  </span>
                )}

                <div className="episode-observation-body">
                  <div className="episode-observation-meta">
                    <span className="episode-observation-time">{clockSeconds(obs.timestamp)}</span>
                    {obs.application && (
                      <span className="episode-observation-app">{obs.application}</span>
                    )}
                  </div>
                  {obs.windowTitle && (
                    <span className="episode-observation-window">{obs.windowTitle}</span>
                  )}

                  {obs.ocrText && (
                    <div className="episode-field">
                      <span className="episode-field-label">{L.logosOcr}</span>
                      <pre className="episode-ocr">{obs.ocrText}</pre>
                    </div>
                  )}

                  {obs.uia != null && (
                    <details className="episode-field episode-uia">
                      <summary>
                        <span className="episode-field-label">{L.logosUia}</span>
                      </summary>
                      <pre className="episode-ocr">{JSON.stringify(obs.uia, null, 2)}</pre>
                    </details>
                  )}
                </div>
              </li>
            ))}
          </ol>
        </section>
      )}

      <footer className="episode-actions">
        <button
          type="button"
          className="episode-action episode-action-primary"
          onClick={() => onAskGaia && onAskGaia(episode)}
        >
          <MessageCircle size={15} aria-hidden="true" />
          {L.logosAskGaia}
        </button>
        <button
          type="button"
          className="episode-action"
          onClick={() => onSavePattern && onSavePattern(episode)}
        >
          <Bookmark size={15} aria-hidden="true" />
          {L.logosSavePattern}
        </button>
      </footer>
    </article>
  );
}
