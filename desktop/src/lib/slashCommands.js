/**
 * Slash commands — typed in the composer, executed locally or forwarded.
 *
 * A command starts with `/` and is dispatched before anything is sent to
 * Gaia Cloud. Local commands (new, stop, help) never leave the device;
 * server-bound commands (retry, undo) are handled by the conversation hook
 * through the callbacks passed to `dispatch`.
 *
 * The command list is the single source of truth for both execution and
 * the autocomplete dropdown shown while typing.
 */

import { L } from './lexicon';

/**
 * The available commands. `scope` decides where the command runs:
 *   - 'local'  → handled here, returns a result string or null
 *   - 'action' → forwarded to the conversation hook via callbacks
 *
 * `aliases` are alternative spellings that resolve to the same command.
 * `hint` is the one-line description shown in the autocomplete dropdown.
 */
export const SLASH_COMMANDS = [
  {
    name: 'new',
    aliases: ['n'],
    scope: 'action',
    action: 'newThread',
    hint: () => L.slashNewHint,
    label: () => L.slashNew,
  },
  {
    name: 'stop',
    aliases: ['s'],
    scope: 'action',
    action: 'cancel',
    hint: () => L.slashStopHint,
    label: () => L.slashStop,
    requiresBusy: true,
  },
  {
    name: 'retry',
    aliases: ['r', 'again'],
    scope: 'action',
    action: 'retryLast',
    hint: () => L.slashRetryHint,
    label: () => L.slashRetry,
  },
  {
    name: 'undo',
    aliases: ['u'],
    scope: 'action',
    action: 'undoLast',
    hint: () => L.slashUndoHint,
    label: () => L.slashUndo,
  },
  {
    name: 'help',
    aliases: ['h', '?'],
    scope: 'local',
    hint: () => L.slashHelpHint,
    label: () => L.slashHelp,
  },
];

/**
 * Parses a raw input string into a command name + arguments.
 * Returns null when the input is not a slash command.
 *
 * @param {string} text
 * @returns {{ name: string, args: string, raw: string } | null}
 */
export function parseSlashCommand(text) {
  const trimmed = String(text || '').trim();
  if (!trimmed.startsWith('/')) return null;
  const withoutSlash = trimmed.slice(1);
  if (!withoutSlash) return null;
  const spaceIndex = withoutSlash.indexOf(' ');
  const name = (spaceIndex === -1 ? withoutSlash : withoutSlash.slice(0, spaceIndex)).toLowerCase();
  const args = spaceIndex === -1 ? '' : withoutSlash.slice(spaceIndex + 1).trim();
  if (!name) return null;
  return { name, args, raw: trimmed };
}

/**
 * Resolves a command name (or alias) to its definition.
 * @param {string} name
 * @returns {object | null}
 */
export function resolveCommand(name) {
  const lower = String(name || '').toLowerCase();
  return SLASH_COMMANDS.find((cmd) => cmd.name === lower || cmd.aliases.includes(lower)) || null;
}

/**
 * Returns the commands matching a prefix, for the autocomplete dropdown.
 * @param {string} prefix — the text after `/`, e.g. "re" → ["retry"]
 * @returns {Array<object>}
 */
export function matchCommands(prefix) {
  const lower = String(prefix || '').toLowerCase();
  if (!lower) return SLASH_COMMANDS;
  return SLASH_COMMANDS.filter(
    (cmd) => cmd.name.startsWith(lower) || cmd.aliases.some((a) => a.startsWith(lower))
  );
}

/**
 * Dispatches a parsed command. Local commands are executed here and return
 * a result string; action commands call the matching callback and return
 * its result (or null when the callback is missing).
 *
 * @param {{ name: string, args: string, raw: string }} parsed
 * @param {object} callbacks — { newThread, cancel, retryLast, undoLast }
 * @param {object} context — { busy, hasMessages }
 * @returns {{ handled: boolean, message?: string, consumed: boolean }}
 *
 * `handled`   — the command was recognised (even if it could not run)
 * `consumed`  — the input should be cleared (command executed or rejected)
 * `message`   — optional feedback string to show as a system note
 */
export function dispatchSlashCommand(parsed, callbacks = {}, context = {}) {
  const cmd = resolveCommand(parsed.name);
  if (!cmd) {
    return { handled: false, consumed: false };
  }

  // Commands that need a running turn or existing messages are rejected
  // with a calm message rather than silently failing.
  if (cmd.requiresBusy && !context.busy) {
    return { handled: true, consumed: true, message: L.slashNothingToStop };
  }

  if (cmd.scope === 'local') {
    if (cmd.name === 'help') {
      return { handled: true, consumed: true, message: buildHelpText() };
    }
    return { handled: true, consumed: true };
  }

  // Action command: forward to the conversation hook.
  const actionFn = callbacks[cmd.action];
  if (typeof actionFn === 'function') {
    const result = actionFn(parsed.args);
    return { handled: true, consumed: true, message: result?.message };
  }

  return { handled: true, consumed: true, message: L.slashNotAvailable };
}

/**
 * Builds the help text listing all commands.
 * @returns {string}
 */
function buildHelpText() {
  const lines = SLASH_COMMANDS.map((cmd) => {
    const aliases = cmd.aliases.length > 0 ? ` (${cmd.aliases.map((a) => `/${a}`).join(', ')})` : '';
    return `/${cmd.name}${aliases} — ${cmd.hint()}`;
  });
  return `${L.slashHelpTitle}\n${lines.join('\n')}`;
}
