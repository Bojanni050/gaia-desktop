/**
 * Desktop actions — Gaia working on this computer, through Desktop Commander.
 *
 * Gaia Cloud executes nothing here. It announces an action on the open turn
 * (a `desktop` frame, the same family as `memory` and `step`), holds the tool
 * call, and waits for this client to report back. The contract is Gaia-Cloud's
 * docs/desktop-capability.md; this file is the client's half of it:
 *
 *   awaiting_confirmation  → ask the person, showing the summary AND the input
 *   approved               → run it locally now (never before)
 *   denied/expired/…       → the prompt goes away
 *
 * What lives here is deliberately plain: policy is the server's (the client may
 * ask for MORE confirmation, never less), and the limits — allowed folders and
 * a command blocklist — are the person's own, set in Settings. Output from the
 * computer is data; it is sent back as such and never acted on.
 */

/** What the server's own cap is; we clip first so we never send more than it keeps. */
export const MAX_OUTPUT_CHARS = 8000;

const TERMINAL = new Set(['denied', 'expired', 'completed', 'failed']);
const KNOWN_SKILLS = new Set(['file-read', 'file-search', 'file-write', 'run-command']);

// How long an availability probe is trusted, so a turn does not start Desktop
// Commander every time. A failure is remembered for a shorter while.
const PROBE_OK_MS = 5 * 60 * 1000;
const PROBE_FAIL_MS = 30 * 1000;
const NOTICE_MS = 8000;
// Desktop Commander gets less time than the server's result window (60 s), so a slow
// tool is reported as a failure instead of letting the turn run into the timeout.
const CALL_TIMEOUT_MS = 45 * 1000;

// --- frames -----------------------------------------------------------------

/** Narrows a relayed `desktop` frame to what the client uses; null when it is not one. */
export function parseDesktopFrame(frame) {
  if (!frame || typeof frame !== 'object') return null;
  if (typeof frame.id !== 'string' || !frame.id) return null;
  if (typeof frame.skill !== 'string' || typeof frame.status !== 'string') return null;
  return {
    id: frame.id,
    skill: frame.skill,
    policy: frame.policy === 'auto' ? 'auto' : 'confirm', // anything unclear fails closed
    status: frame.status,
    summary: typeof frame.summary === 'string' ? frame.summary : '',
    input: frame.input && typeof frame.input === 'object' ? frame.input : {},
    conversationId: typeof frame.conversationId === 'string' ? frame.conversationId : null,
  };
}

// --- limits -----------------------------------------------------------------

/**
 * Lower-cased, forward-slashed, with `.` and `..` resolved. Returns null for
 * anything that is not an absolute path (relative, `~`, empty) or that climbs
 * above its own root: such a path has no safe meaning to compare.
 */
export function normalizePath(path) {
  if (typeof path !== 'string') return null;
  const raw = path.trim().replace(/\\/g, '/');
  if (!raw || raw.includes('\0')) return null;
  const drive = /^([a-zA-Z]):(\/|$)/.exec(raw);
  const unc = raw.startsWith('//');
  if (!drive && !unc && !raw.startsWith('/')) return null;
  const root = drive ? `${drive[1].toLowerCase()}:` : unc ? '//' : '';
  const rest = drive ? raw.slice(2) : unc ? raw.slice(2) : raw;
  const parts = [];
  for (const part of rest.split('/')) {
    if (!part || part === '.') continue;
    if (part === '..') {
      if (parts.length === 0) return null;
      parts.pop();
    } else {
      parts.push(part);
    }
  }
  // Windows and UNC paths are case-insensitive; compare them that way everywhere.
  const body = drive || unc ? parts.join('/').toLowerCase() : parts.join('/');
  return `${root}/${body}`.replace(/\/$/, '') || '/';
}

export function isWithinAllowed(path, allowedDirs) {
  const target = normalizePath(path);
  if (!target) return false;
  return (allowedDirs || []).some((dir) => {
    const base = normalizePath(dir);
    if (!base) return false;
    return target === base || target.startsWith(base === '/' ? '/' : `${base}/`);
  });
}

const squash = (text) => String(text).toLowerCase().replace(/\s+/g, ' ');

/** The first blocklist entry the command contains, or null. Case- and spacing-insensitive. */
export function blockedCommandHit(command, blocklist) {
  const text = squash(command);
  for (const entry of blocklist || []) {
    const needle = squash(entry).trim();
    if (needle && text.includes(needle)) return entry.trim();
  }
  return null;
}

/**
 * Whether this device's own limits allow an action. `{ ok: true }` or
 * `{ ok: false, reason }` with a short reason for Gaia (and the person).
 * An unknown skill is never allowed: there is nothing to run.
 */
export function checkLimits(skill, input, desktopSettings) {
  const settings = desktopSettings || {};
  const i = input && typeof input === 'object' ? input : {};
  if (!KNOWN_SKILLS.has(skill)) return { ok: false, reason: `Onbekende vaardigheid: ${skill}` };
  if (skill === 'run-command') {
    if (typeof i.command !== 'string' || !i.command.trim()) {
      return { ok: false, reason: 'Er is geen commando opgegeven.' };
    }
    const hit = blockedCommandHit(i.command, settings.blockedCommands);
    if (hit) return { ok: false, reason: `Dit commando staat op de blokkadelijst (${hit}).` };
    return { ok: true };
  }
  if (typeof i.path !== 'string' || !i.path.trim()) {
    return { ok: false, reason: 'Er is geen pad opgegeven.' };
  }
  if (!normalizePath(i.path)) {
    return { ok: false, reason: 'Het pad moet een volledig pad zijn, zonder “..” buiten de map.' };
  }
  if (!isWithinAllowed(i.path, settings.allowedDirs)) {
    return { ok: false, reason: 'Dit pad ligt buiten de toegestane mappen.' };
  }
  return { ok: true };
}

// --- Desktop Commander ------------------------------------------------------

const FOLDER_SEARCH_DEPTH = 3;
const MAX_FOLDER_HITS = 50;

/**
 * The `[DIR]` lines of a `list_directory` result whose own name contains `query`, turned into full
 * paths (the listing is relative to `base`), so Gaia can use them directly.
 */
export function foldersMatching(text, base, query) {
  const needle = String(query ?? '').trim().toLowerCase();
  const root = String(base ?? '').replace(/[\\/]+$/, '');
  const hits = [];
  for (const line of String(text ?? '').split('\n')) {
    const match = /^\s*\[DIR\]\s+(.+?)\s*$/.exec(line);
    if (!match) continue;
    const relative = match[1];
    const name = relative.split(/[\\/]/).pop().toLowerCase();
    if (needle && name.includes(needle)) hits.push(`[DIR] ${root}\\${relative.replace(/\//g, '\\')}`);
    if (hits.length >= MAX_FOLDER_HITS) break;
  }
  return hits.length > 0
    ? hits.join('\n')
    : `Geen map met "${query}" in de naam gevonden, tot ${FOLDER_SEARCH_DEPTH} niveaus onder ${base}.`;
}

/**
 * The Desktop Commander tool calls that carry out a skill. A skill may take more
 * than one call (a search by name and by content); results are joined in order.
 */
export function toolCallsFor(skill, input) {
  const i = input || {};
  switch (skill) {
    case 'file-read':
      return [{ tool: 'read_file', args: { path: i.path } }];
    case 'file-search': {
      const query = String(i.query ?? '');
      if (!query.trim()) return [];
      const base = { path: i.path, pattern: query, literalSearch: true, maxResults: 50, timeout_ms: 10000 };
      // Names by default: fast, and fine for a whole drive. Reading the text of every file under a
      // drive is slow and sends far more of the person's data to Gaia, so contents are opt-in.
      const where = String(i.where ?? 'names').toLowerCase();
      // Desktop Commander's name search only matches files, never folders, so finding a folder
      // means listing a few levels down and keeping the folders whose name matches.
      if (where === 'folders') {
        return [
          {
            tool: 'list_directory',
            args: { path: i.path, depth: FOLDER_SEARCH_DEPTH },
            transform: (text) => foldersMatching(text, i.path, query),
          },
        ];
      }
      const types = where === 'contents' ? ['content'] : where === 'both' ? ['files', 'content'] : ['files'];
      return types.map((searchType) => ({ tool: 'start_search', args: { ...base, searchType } }));
    }
    case 'file-write':
      return [{ tool: 'write_file', args: { path: i.path, content: String(i.content ?? ''), mode: 'rewrite' } }];
    case 'run-command':
      return [{ tool: 'start_process', args: { command: i.command, timeout_ms: 15000 } }];
    default:
      return [];
  }
}

/** The text of an MCP `tools/call` result, and whether the tool itself called it an error. */
export function readToolResult(result) {
  const content = Array.isArray(result?.content) ? result.content : [];
  const text = content
    .filter((part) => part && part.type === 'text' && typeof part.text === 'string')
    .map((part) => part.text)
    .join('\n');
  return { text, isError: result?.isError === true };
}

export function truncateOutput(text, max = MAX_OUTPUT_CHARS) {
  const value = String(text ?? '');
  return value.length > max ? `${value.slice(0, max)}…` : value;
}

// --- controller -------------------------------------------------------------

/**
 * One controller per window. It holds the prompts the person still has to
 * answer and the bookkeeping that makes every frame safe to receive twice.
 *
 * `request`  – serverApi.request (Bearer-authenticated by Rust, like every call)
 * `callTool` – mcpApi.callTool(serverId, tool, args)
 * `listTools`– mcpApi.listTools(serverId), used only to see whether it runs
 * `getSettings` – settingsApi.get
 */
export function createDesktopActions({ request, callTool, listTools, getSettings, getPlaces = async () => ({}), now = Date.now }) {
  // id → 'prompted' | 'decided' | 'running' | 'done'. A frame for an id that is
  // past its prompt is ignored; 'done' is kept so a late repeat can't re-run it.
  const phase = new Map();
  const prompts = new Map(); // id → parsed action awaiting the person
  const localApproved = new Set(); // ids the person approved HERE: confirm-policy actions run only for these
  const running = new Map(); // id → { aborted }
  const listeners = new Set();
  let snapshot = { prompts: [], notice: null };
  let noticeTimer = null;
  let probe = null; // { serverId, ok, at }
  let generation = 0; // bumped by abortAll so late results of an old turn are dropped

  const publish = () => {
    snapshot = { prompts: [...prompts.values()], notice: snapshot.notice };
    listeners.forEach((fn) => fn());
  };
  const setNotice = (text) => {
    clearTimeout(noticeTimer);
    snapshot = { ...snapshot, notice: text };
    listeners.forEach((fn) => fn());
    if (text) {
      noticeTimer = setTimeout(() => setNotice(null), NOTICE_MS);
    }
  };

  // A 404/409 means the action is already gone or answered; any other failure of
  // an answer cannot be fixed by showing the person an error either. Either way
  // the prompt is gone and nothing is shown.
  const send = async (path, body) => {
    try {
      await request({ method: 'post', path, body });
    } catch (_) {
      /* already expired, answered, or unreachable: nothing to show */
    }
  };
  const postDecision = (id, approved) =>
    send(`desktop/actions/${encodeURIComponent(id)}/decision`, { approved });
  const postResult = (id, ok, output) =>
    send(`desktop/actions/${encodeURIComponent(id)}/result`, { ok, output: truncateOutput(output) });

  const dropPrompt = (id) => {
    if (prompts.delete(id)) publish();
  };

  async function execute(action) {
    const { id, skill, input } = action;
    const run = { aborted: false };
    running.set(id, run);
    phase.set(id, 'running');
    const myGeneration = generation;
    const finish = async (ok, output) => {
      running.delete(id);
      phase.set(id, 'done');
      // Interrupted, expired or abandoned meanwhile: the server no longer waits for this.
      if (run.aborted || myGeneration !== generation) return;
      await postResult(id, ok, output);
    };

    let settings;
    try {
      settings = (await getSettings())?.desktop || {};
    } catch (_) {
      return finish(false, 'De instellingen van deze computer waren niet te lezen.');
    }
    // The limits are checked again at the moment of running, whatever was checked at the prompt.
    const limits = checkLimits(skill, input, settings);
    if (!limits.ok) return finish(false, limits.reason);
    const calls = toolCallsFor(skill, input);
    if (calls.length === 0) return finish(false, 'Er viel niets uit te voeren met deze invoer.');
    if (!settings.enabled || !settings.serverId) {
      return finish(false, 'Werken op deze computer staat uit.');
    }

    try {
      const outputs = [];
      for (const { tool, args, transform } of calls) {
        if (run.aborted) return finish(false, 'Afgebroken.');
        const result = await withTimeout(callTool(settings.serverId, tool, args), CALL_TIMEOUT_MS);
        const { text, isError } = readToolResult(result);
        if (isError) return finish(false, text || 'De actie mislukte.');
        outputs.push(transform ? transform(text) : text);
      }
      return finish(true, outputs.filter(Boolean).join('\n---\n') || '(geen uitvoer)');
    } catch (_) {
      probe = null; // it may have stopped; look again before the next turn
      setNotice('Desktop Commander is nu niet bereikbaar. Ik werk even niet op deze computer.');
      return finish(false, 'Desktop Commander is niet beschikbaar.');
    }
  }

  async function handleFrame(raw) {
    const action = parseDesktopFrame(raw);
    if (!action) return;
    const { id, status } = action;
    const known = phase.get(id);

    if (TERMINAL.has(status)) {
      // This is how a timeout reaches the person: the prompt goes, a running action is let go.
      dropPrompt(id);
      const run = running.get(id);
      if (run) run.aborted = true;
      phase.set(id, 'done');
      localApproved.delete(id);
      return;
    }
    if (known === 'done') return;

    if (status === 'awaiting_confirmation') {
      if (known) return; // the prompt is already up, or already answered
      let settings = {};
      try {
        settings = (await getSettings())?.desktop || {};
      } catch (_) { /* no settings: nothing is allowed */ }
      const limits = checkLimits(action.skill, action.input, settings);
      if (!limits.ok) {
        // Outside the person's own limits: no prompt, no second question. A refusal is the
        // only thing the server can be told for an action that was never approved.
        phase.set(id, 'done');
        setNotice(`Ik heb een actie geweigerd: ${limits.reason}`);
        await postDecision(id, false);
        return;
      }
      phase.set(id, 'prompted');
      prompts.set(id, action);
      publish();
      return;
    }

    if (status === 'approved') {
      if (known === 'running') return;
      dropPrompt(id);
      // Never less confirmation than the server asked for: a confirm action that the
      // person did not approve in this window does not run, whatever the frame says.
      if (action.policy !== 'auto' && !localApproved.has(id)) {
        phase.set(id, 'done');
        await postResult(id, false, 'Niet goedgekeurd op deze computer; er is niets uitgevoerd.');
        return;
      }
      localApproved.delete(id);
      await execute(action);
    }
  }

  /** The person's answer to a prompt. Safe to call twice; only the first counts. */
  async function decide(id, approved) {
    if (!prompts.has(id) || phase.get(id) !== 'prompted') return;
    dropPrompt(id);
    phase.set(id, 'decided');
    if (approved) localApproved.add(id);
    else phase.set(id, 'done');
    await postDecision(id, approved);
  }

  /**
   * The turn ended or was interrupted, or the window is closing: close every prompt and
   * let running actions go. The server expires what is still open as soon as the stream closes.
   */
  function abortAll() {
    generation += 1;
    running.forEach((run) => {
      run.aborted = true;
    });
    prompts.clear();
    phase.clear();
    localApproved.clear();
    publish();
  }

  /**
   * Whether this turn may offer `desktop: true`: switched on in Settings, with at least one
   * allowed folder, and Desktop Commander actually answering.
   */
  async function isAvailable() {
    let settings;
    try {
      settings = (await getSettings())?.desktop;
    } catch (_) {
      return false;
    }
    if (!settings?.enabled || !settings.serverId) return false;
    if (!Array.isArray(settings.allowedDirs) || !settings.allowedDirs.some((dir) => normalizePath(dir))) {
      return false;
    }
    const fresh =
      probe && probe.serverId === settings.serverId && now() - probe.at < (probe.ok ? PROBE_OK_MS : PROBE_FAIL_MS);
    if (fresh) return probe.ok;
    let ok = false;
    try {
      await withTimeout(listTools(settings.serverId), 15000);
      ok = true;
    } catch (_) {
      ok = false;
    }
    probe = { serverId: settings.serverId, ok, at: now() };
    return ok;
  }

  /**
   * The allowed folders, as full paths, for telling Gaia where she may look. Same list the limits
   * are checked against; entries that are not usable paths are left out.
   */
  async function allowedRoots() {
    try {
      const dirs = (await getSettings())?.desktop?.allowedDirs;
      return (Array.isArray(dirs) ? dirs : []).filter((dir) => normalizePath(dir)).map((dir) => String(dir).trim());
    } catch (_) {
      return [];
    }
  }

  /**
   * Desktop, Documents and Downloads as Windows has them (OneDrive may have moved them to another
   * drive), limited to what is inside the allowed folders: she is never pointed at a place she
   * may not use.
   */
  async function knownPlaces() {
    try {
      const [places, dirs] = await Promise.all([getPlaces(), allowedRoots()]);
      const out = {};
      for (const key of ['desktop', 'documents', 'downloads']) {
        const path = places?.[key];
        if (typeof path === 'string' && normalizePath(path) && isWithinAllowed(path, dirs)) out[key] = path;
      }
      return out;
    } catch (_) {
      return {};
    }
  }

  return {
    handleFrame,
    decide,
    abortAll,
    isAvailable,
    allowedRoots,
    knownPlaces,
    dismissNotice: () => setNotice(null),
    subscribe: (fn) => {
      listeners.add(fn);
      return () => listeners.delete(fn);
    },
    getSnapshot: () => snapshot,
  };
}

function withTimeout(promise, ms) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('timeout')), ms);
  });
  return Promise.race([Promise.resolve(promise), timeout]).finally(() => clearTimeout(timer));
}
