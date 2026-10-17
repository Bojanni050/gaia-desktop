/**
 * Understanding — the human Absolute Override, as a right-hand drawer.
 *
 * These are the derived statements Logos is still weighing (hypotheses,
 * candidate models, open questions, relationships); the person confirms one,
 * answers a question, or lets it go. Only this verdict reaches `confirmed` — no
 * model, no accumulation of evidence on its own. Each row renders as an
 * EpistemicReviewCard.
 *
 * The drawer holds three lists, one per tab: "Te beoordelen" — what still
 * waits for a verdict — "Bevestigd", the statements the person has taken as
 * their own, which is their record to read back — and "Heroverwegen", the
 * statements that were let go. A rejected statement is a hard quarantine whose
 * only way back is a stated reason, so it is a list of its own instead of a
 * form sitting in the middle of the cards that still ask something of the
 * person. All three are filled from the one list the server returns, so
 * switching never costs a round trip.
 *
 * Closed, the drawer is only a 16px edge on the right of the window with a
 * golden dot in its middle: it pulses softly while there is something in
 * Understanding the person has not seen yet, and rests when there is not. The
 * list is fetched in the background (at mount and on an interval) so the dot
 * knows what is there even while the drawer is closed; what the open tab shows
 * counts as seen (and arriving items while it is open are marked seen too), so
 * the dot never pulses under the person's eyes — a count on the other tab
 * still says what is waiting there.
 */
import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { X } from 'lucide-react';
import { cognitionApi } from '../server/api';
import { L } from '../lib/lexicon';
import EpistemicReviewCard from './EpistemicReviewCard';

const SEEN_KEY = 'gaia.understanding.seen';
const WIDTH_KEY = 'gaia.understanding.width';
// The drawer's three lists. `review` is everything that still waits for a
// verdict; `confirmed` is what the person has taken as their own — their own
// record, read back; `reopen` is the rejected quarantine, whose only way back
// is a stated reason.
const TAB_REVIEW = 'review';
const TAB_CONFIRMED = 'confirmed';
const TAB_REOPEN = 'reopen';
const TAB_ORDER = [TAB_REVIEW, TAB_CONFIRMED, TAB_REOPEN];

/** The one place that decides which list a statement belongs to. A lapsed
 * statement belongs with the things that were let go: it is not waiting for a
 * round, and counting it as waiting is what made the old figure so large. */
const listOf = (item) =>
  item.status === 'rejected' || item.status === 'expired'
    ? TAB_REOPEN
    : item.status === 'confirmed' ? TAB_CONFIRMED : TAB_REVIEW;

/**
 * gaia-api serves what is under review as a round (reviewRound.js): at most a
 * handful of statements that carry weight (macro, or a counter-hypothesis) and
 * have built up enough evidence. The rest waits for a later round and is only
 * counted. A statement that waits is not "on screen", so it never makes the dot
 * pulse. A server that sends no round (`in_round` absent) shows everything.
 */
const isWaiting = (it) => listOf(it) === TAB_REVIEW && it.in_round === false;
const RAIL_WIDTH = 16;
// How long the drawer and the rail button take to slide; matches `--understanding-slide` in styles.css.
const SLIDE_MS = 450;

/** The drawer never narrows below this (unless the window itself is smaller). */
export const MIN_WIDTH = 320; // 20rem
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
  // The drawer and the rail button slide as ONE movement: `shown` flips both at the same moment, and
  // the drawer stays mounted until its slide out has finished. `resizing` switches the slide off while
  // the edge is being dragged, so the button follows the pointer instead of trailing 0.45 s behind.
  const [mounted, setMounted] = useState(false);
  const [shown, setShown] = useState(false);
  const [resizing, setResizing] = useState(false);
  const [items, setItems] = useState(null); // null = not loaded yet
  const [error, setError] = useState(null);
  const [busyId, setBusyId] = useState(null);
  const [tab, setTab] = useState(TAB_REVIEW);
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

  // Whatever the open tab shows counts as seen — including items that arrive
  // from a background refresh while the person is reading, and everything a
  // tab shown earlier showed. The dot is about what has ever been on screen,
  // never about the tab that happens to be open; what waits on the other tab
  // is said by its count.
  useEffect(() => {
    if (!open || !items) return;
    const visible = items.filter((it) => listOf(it) === tab && !isWaiting(it));
    const next = new Set([...seen, ...visible.map((it) => it.id)]);
    if (next.size === seen.size) return;
    writeSeen(next);
    setSeen(next);
  }, [open, items, tab, seen]);

  useEffect(() => {
    if (open) {
      setMounted(true);
      // Two frames at the hidden position first, so the slide has a start to move from.
      let second = 0;
      const first = requestAnimationFrame(() => {
        second = requestAnimationFrame(() => setShown(true));
      });
      return () => {
        cancelAnimationFrame(first);
        cancelAnimationFrame(second);
      };
    }
    setShown(false);
    const timer = setTimeout(() => setMounted(false), SLIDE_MS);
    return () => clearTimeout(timer);
  }, [open]);

  useEffect(() => {
    if (!open) return undefined;
    const onKey = (e) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [open]);

  const hasNew = useMemo(
    () => Boolean(items && items.some((it) => !isWaiting(it) && !seen.has(it.id))),
    [items, seen]
  );

  // Every tab is filled from the one list the server returned, so switching
  // never costs a round trip.
  const { lists, waiting } = useMemo(() => {
    const grouped = { [TAB_REVIEW]: [], [TAB_CONFIRMED]: [], [TAB_REOPEN]: [] };
    let waitingCount = 0;
    (items || []).forEach((it) => {
      if (isWaiting(it)) {
        waitingCount += 1;
        return;
      }
      grouped[listOf(it)].push(it);
    });
    return { lists: grouped, waiting: waitingCount };
  }, [items]);
  const shownItems = lists[tab];
  // Read at render time, the same way L is read everywhere else.
  const tabCopy = {
    [TAB_REVIEW]: { label: L.cognitionTabReview, empty: L.cognitionEmpty },
    [TAB_CONFIRMED]: { label: L.cognitionTabConfirmed, empty: L.cognitionConfirmedEmpty },
    [TAB_REOPEN]: { label: L.cognitionTabReopen, empty: L.cognitionReopenEmpty },
  };

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

  // Answering an open question is not a verdict: no status changes, nothing is
  // mirrored — the answer is recorded and the question leaves the list (the
  // server drops it from the next list too).
  const handleAnswer = (item, answer) =>
    run(item, () => cognitionApi.answer(item.id, answer),
      () => setItems((prev) => (prev || []).filter((it) => it.id !== item.id)));

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
    setResizing(true);
    const onMove = (ev) => {
      setWidth(clampWidth(window.innerWidth - RAIL_WIDTH - ev.clientX, window.innerWidth));
    };
    const onUp = () => {
      setResizing(false);
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
        className={`understanding-rail${hasNew ? ' has-new' : ''}${resizing ? ' is-resizing' : ''}`}
        style={shown ? { right: RAIL_WIDTH + effectiveWidth } : undefined}
        onClick={() => setOpen((v) => !v)}
        aria-expanded={open}
        aria-label={hasNew ? L.cognitionRailNew : L.cognitionRailOpen}
      >
        <span className="understanding-dot" aria-hidden="true" />
      </button>

      {mounted && (
        <aside
          className={`understanding-drawer${shown ? ' in' : ''}${resizing ? ' is-resizing' : ''}`}
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
          <div className="understanding-tabs" role="tablist" aria-label={L.cognitionTitle}>
            {TAB_ORDER.map((id) => (
              <button
                key={id}
                type="button"
                role="tab"
                id={`understanding-tab-${id}`}
                aria-controls="understanding-tabpanel"
                aria-selected={tab === id}
                className={`understanding-tab${tab === id ? ' active' : ''}`}
                onClick={() => setTab(id)}
              >
                {tabCopy[id].label}
                {lists[id].length > 0 && (
                  <span className="understanding-tab-count">{lists[id].length}</span>
                )}
              </button>
            ))}
          </div>
          <div
            className="understanding-drawer-body"
            id="understanding-tabpanel"
            role="tabpanel"
            aria-labelledby={`understanding-tab-${tab}`}
          >
            {error && <div className="sidebar-section-error">{error}</div>}
            {items === null ? (
              <p className="sidebar-section-hint">{L.cognitionLoading}</p>
            ) : shownItems.length === 0 ? (
              <p className="sidebar-section-hint">{tabCopy[tab].empty}</p>
            ) : (
              <>
                {tab === TAB_REVIEW && <p className="sidebar-section-hint">{L.cognitionHint}</p>}
                {shownItems.map((item) => (
                  <EpistemicReviewCard
                    key={item.id}
                    item={item}
                    busy={busyId === item.id}
                    onConfirm={handleConfirm}
                    onReject={handleReject}
                    onReopen={handleReopen}
                    onAnswer={handleAnswer}
                  />
                ))}
              </>
            )}
            {items !== null && tab === TAB_REVIEW && waiting > 0 && (
              <p className="sidebar-section-hint">{L.cognitionWaiting.replace('{n}', String(waiting))}</p>
            )}
          </div>
        </aside>
      )}
    </>
  );
}
