/**
 * EpisodeCard — one stretch of activity Kairos recognised, as a card.
 *
 * The whole point of the card is an epistemic separation the eye can read in
 * one glance: the interpretation is the loud, human sentence at the top; the
 * observations capture-rs actually recorded are quiet, collapsed, and only
 * appear on request. Nothing here is allowed to blur the two — the badge says
 * "Episode / Interpretation", the evidence section says "raw observations" and
 * carries a note that Logos did not write it.
 *
 * Evidence is loaded lazily: an episode from the list carries none (one cheap
 * call), so opening the accordion fetches the raw observations through
 * `loadEvidence(episode.id)` — the audit path from interpretation to fact. If
 * the episode already carries `observations` (the mock/demo path), no fetch
 * runs; `loadEvidence` is optional.
 */
import React, { useState } from 'react';
import {
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

export default function EpisodeCard({ episode, onAskGaia, loadEvidence }) {
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [loaded, setLoaded] = useState(null); // fetched observations, or null before load
  const [evidenceError, setEvidenceError] = useState(null);
  const [evidenceLoading, setEvidenceLoading] = useState(false);

  const context = Array.isArray(episode.context) ? episode.context : [];
  const inlineObservations = Array.isArray(episode.observations) ? episode.observations : [];
  const observations = loaded !== null ? loaded : inlineObservations;
  const panelId = `episode-evidence-${episode.id}`;

  async function toggleEvidence() {
    const next = !evidenceOpen;
    setEvidenceOpen(next);
    // Fetch once, on first open, and only when there is nothing inline and a
    // loader was wired.
    if (
      next &&
      loaded === null &&
      inlineObservations.length === 0 &&
      typeof loadEvidence === 'function'
    ) {
      setEvidenceLoading(true);
      setEvidenceError(null);
      try {
        const rows = await loadEvidence(episode.id);
        setLoaded(Array.isArray(rows) ? rows : []);
      } catch (_) {
        setEvidenceError(L.logosEvidenceFailed);
        setLoaded([]);
      } finally {
        setEvidenceLoading(false);
      }
    }
  }

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
          </div>
          {episode.title && <h3 className="episode-title">{episode.title}</h3>}
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
        onClick={toggleEvidence}
      >
        <ScrollText size={14} aria-hidden="true" />
        <span>{L.logosEvidenceToggle}</span>
        {(observations.length > 0) && (
          <span className="episode-evidence-count">
            ({observations.length} {observations.length === 1 ? L.logosCapture : L.logosCaptures})
          </span>
        )}
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

          {evidenceLoading && <p className="logos-empty">{L.logosLoading}</p>}
          {evidenceError && <div className="logos-error">{evidenceError}</div>}
          {!evidenceLoading && !evidenceError && observations.length === 0 && (
            <p className="logos-empty">{L.logosEvidenceEmpty}</p>
          )}

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
                    {obs.timestamp && (
                      <span className="episode-observation-time">{clockSeconds(obs.timestamp)}</span>
                    )}
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
      </footer>
    </article>
  );
}
