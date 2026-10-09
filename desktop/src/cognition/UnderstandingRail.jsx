/**
 * Understanding — the human Absolute Override, as a right-hand drawer.
 *
 * These are the derived statements Logos is still weighing (hypotheses,
 * candidate models, open questions, relationships); the person confirms one or
 * lets it go. Only this verdict reaches `confirmed` — no model, no accumulation
 * of evidence on its own. Each row renders as an EpistemicReviewCard.
 *
 * Closed, the drawer is only a 16px edge on the right of the window with a
 * golden dot in its middle: it pulses softly while there is something in
 * Understanding the person has not seen yet, and rests when there is not. The
 * list is fetched in the background (at mount and on an interval) so the dot
 * knows what is there even while the drawer is closed; opening it marks
 * everything currently shown as seen, and arriving items while it is open are
 * marked seen too, so the dot never pulses under the person's eyes.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { cognitionApi } from '../server/api';
import { L } from '../lib/lexicon';
import EpistemicReviewCard from './EpistemicReviewCard';

const SEEN_KEY = 'gaia.understanding.seen';
const WIDTH_KEY = 'gaia.understanding.width';
const RAIL_WIDTH = 16;

/** The drawer never narrows below this (unless the window itself is smaller). */
export const MIN_WIDTH = 300;
/** The drawer never grows past this share of the window — the request's max 33%. */
export const MAX_WIDTH_RATIO = 0.33;

/**
 * Clamps a drawer width to [min(MIN_WIDTH, max), max] where max is 33% of the
 * viewport, so the drawer can be dragged out but never past a third of the
 * screen — and at a very narrow window it simply fills that third.
 */
export function clampWidth(value, viewportWidth) {
  const max = viewportWidth * MAX_WIDTH_RATIO;
  const min = Math.min(MIN_WIDTH, max);
  const width = Number(value);
  const safe = Number.isFinite(width) ? width : min;
  return Math.round(Math.min(Math.max(safe, min), max));
}

/** Ids of the statements already seen, or an empty set when none/unavailable. */
function readSeen() {
  try {
    const parsed = JSON.parse(localStorage.getItem(SEEN_KEY) || '[]');
    return new Set(Array.isArray(parsed) ? parsed : []);
  } catch (_) {
    return new Set();
  }
}

function writeSeen(ids) {
  try {
    localStorage.setItem(SEEN_KEY, JSON.stringify([...ids]));
  } catch (_) {
    /* storage unavailable — the dot simply won't remember across launches */
  }
}

/** The drawer width the person dragged to last, or null when never resized. */
function readWidth() {
  try {
    const raw = localStorage.getItem(WIDTH_KEY);
    const n = raw == null ? null : Number(raw);
    return Number.isFinite(n) ? n : null;
  } catch (_) {
    return null;
  }
}

function writeWidth(value) {
  try {
    localStorage.setItem(WIDTH_KEY, String(value));
  } catch (_) {
    /* storage unavailable — the width simply won't remember across launches */
  }
}

/**
 * @param {{ pollMs?: number }} props `pollMs` is how often the closed drawer
 *   refreshes the list behind the dot; 0 disables the interval (tests).
 */
export default function UnderstandingRail({ pollMs = 45000 } = {}) {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState(null); // null = not loaded yet
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);
  const [seen, setSeen] = useState(readSeen);
  const [width, setWidth] = useState(readWidth); // null = default (30vw, max 460)
  const [viewport, setViewport] = useState(() =>
    typeof window === 'undefined' ? 1024 : window.innerWidth
  );

  const load = useCallback(
    () =>
      cognitionApi
        .list()
        .then((list) => {
          setItems(list);
          setError(null);
        })
        .catch(() => {
          setItems([]);
          setError(L.cognitionLoadFailed);
        }),
    []
  );

  useEffect(() => {
    load();
    if (!pollMs) return undefined;
    const timer = setInterval(load, pollMs);
    return () => clearInterval(timer);
  }, [load, pollMs]);

  // Whatever is on screen while the drawer is open counts as seen — including
  // items that arrive from a background refresh while the person is reading.
  useEffect(() => {
    if (!open || !items) return;
    const next = new Set(items.map((it) => it.id));
    writeSeen(next);
    setSeen(next);
  }, [open, items]);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  const hasNew = useMemo(
    () => Boolean(items && items.some((it) => !seen.has(it.id))),
    [items, seen]
  );

  const afterAction = (id, updated) =>
    setItems((prev) => (prev || []).map((it) => (it.id === id ? { ...it, ...(updated || {}) } : it)));

  const run = async (item, action, apply) => {
    if (busyId) return;
    setBusyId(item.id);
    setError(null);
    try {
      const response = await action();
      apply(response);
    } catch (_) {
      setError(L.cognitionActionFailed);
    } finally {
      setBusyId(null);
    }
  };

  const handleConfirm = (item, { rationale, statement } = {}) =>
    run(item, () => cognitionApi.confirm(item.id, { rationale, statement }),
      (response) => afterAction(item.id, { ...((response && response.body) || {}), status: 'confirmed' }));

  const handleReject = (item) =>
    run(item, () => cognitionApi.reject(item.id, L.cognitionRejected),
      () => afterAction(item.id, { status: 'rejected' }));

  const handleReopen = (item, reason) =>
    run(item, () => cognitionApi.reopen(item.id, reason),
      (response) => afterAction(item.id, { ...((response && response.body) || {}), status: 'testing' }));

  // Keep the clamp honest when the window shrinks: the drawer can be dragged
  // out to at most a third of the screen, and narrows with the window if it
  // was wider than that third.
  useEffect(() => {
    const onResize = () => setViewport(window.innerWidth);
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  const effectiveWidth = clampWidth(
    width == null ? Math.min(viewport * 0.3, 460) : width,
    viewport
  );

  // The latest width, read on pointerup to persist what was dragged to (the
  // state closure inside the listener would be stale).
  const widthRef = useRef(effectiveWidth);
  widthRef.current = effectiveWidth;

  const startResize = useCallback((e) => {
    e.preventDefault();
    const previousSelect = document.body.style.userSelect;
    document.body.style.userSelect = 'none';
    const onMove = (ev) => {
      setWidth(clampWidth(window.innerWidth - RAIL_WIDTH - ev.clientX, window.innerWidth));
    };
    const onUp = () => {
      document.body.style.userSelect = previousSelect;
      window.removeEventListener('pointermove', onMove);
      window.removeEventListener('pointerup', onUp);
      writeWidth(clampWidth(widthRef.current, window.innerWidth));
    };
    window.addEventListener('pointermove', onMove);
    window.addEventListener('pointerup', onUp);
  }, []);

  return (
    <>
      <button
        type="button"
        className={`understanding-rail${hasNew ? ' has-new' : ''}`}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-label={hasNew ? L.cognitionRailNew : L.cognitionRailOpen}
      >
        <span className="understanding-dot" aria-hidden="true" />
      </button>

      {open && (
        <aside
          className="understanding-drawer"
          style={{ width: `${effectiveWidth}px` }}
          aria-label={L.cognitionTitle}
        >
          <div
            className="understanding-resize"
            role="separator"
            aria-orientation="vertical"
            aria-label={L.cognitionResize}
            onPointerDown={startResize}
          />
          <div className="understanding-drawer-head">
            <h2>{L.cognitionTitle}</h2>
            <button
              className="understanding-drawer-close"
              onClick={() => setOpen(false)}
              aria-label={L.cognitionClose}
            >
              <X size={16} />
            </button>
          </div>
          <div className="understanding-drawer-body">
            {error && <div className="sidebar-section-error">{error}</div>}
            {items === null ? (
              <p className="sidebar-section-hint">{L.cognitionLoading}</p>
            ) : items.length === 0 ? (
              <p className="sidebar-section-hint">{L.cognitionEmpty}</p>
            ) : (
              <>
                <p className="sidebar-section-hint">{L.cognitionHint}</p>
                {items.map((item) => (
                  <EpistemicReviewCard
                    key={item.id}
                    item={item}
                    busy={busyId === item.id}
                    onConfirm={handleConfirm}
                    onReject={handleReject}
                    onReopen={handleReopen}
                  />
                ))}
              </>
            )}
          </div>
        </aside>
      )}
    </>
  );
}
