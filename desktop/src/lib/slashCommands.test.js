import { describe, it, expect, vi } from 'vitest';
import {
  SLASH_COMMANDS,
  parseSlashCommand,
  resolveCommand,
  matchCommands,
  dispatchSlashCommand,
} from './slashCommands';

describe('slashCommands: parseSlashCommand', () => {
  it('returns null for ordinary text', () => {
    expect(parseSlashCommand('hallo gaia')).toBeNull();
    expect(parseSlashCommand('')).toBeNull();
    expect(parseSlashCommand('   ')).toBeNull();
  });

  it('returns null for a bare slash', () => {
    expect(parseSlashCommand('/')).toBeNull();
    expect(parseSlashCommand('/ ')).toBeNull();
  });

  it('parses a command without arguments', () => {
    expect(parseSlashCommand('/new')).toEqual({ name: 'new', args: '', raw: '/new' });
    expect(parseSlashCommand('/stop')).toEqual({ name: 'stop', args: '', raw: '/stop' });
  });

  it('parses a command with arguments', () => {
    expect(parseSlashCommand('/model gpt-4')).toEqual({ name: 'model', args: 'gpt-4', raw: '/model gpt-4' });
  });

  it('normalises the command name to lowercase', () => {
    expect(parseSlashCommand('/NEW')).toEqual({ name: 'new', args: '', raw: '/NEW' });
    expect(parseSlashCommand('/Stop')).toEqual({ name: 'stop', args: '', raw: '/Stop' });
  });

  it('trims whitespace', () => {
    expect(parseSlashCommand('  /help  ')).toEqual({ name: 'help', args: '', raw: '/help' });
  });
});

describe('slashCommands: resolveCommand', () => {
  it('resolves by name', () => {
    expect(resolveCommand('new')?.name).toBe('new');
    expect(resolveCommand('stop')?.name).toBe('stop');
    expect(resolveCommand('retry')?.name).toBe('retry');
    expect(resolveCommand('undo')?.name).toBe('undo');
    expect(resolveCommand('help')?.name).toBe('help');
  });

  it('resolves by alias', () => {
    expect(resolveCommand('n')?.name).toBe('new');
    expect(resolveCommand('s')?.name).toBe('stop');
    expect(resolveCommand('r')?.name).toBe('retry');
    expect(resolveCommand('again')?.name).toBe('retry');
    expect(resolveCommand('u')?.name).toBe('undo');
    expect(resolveCommand('h')?.name).toBe('help');
    expect(resolveCommand('?')?.name).toBe('help');
  });

  it('is case-insensitive', () => {
    expect(resolveCommand('NEW')?.name).toBe('new');
    expect(resolveCommand('Retry')?.name).toBe('retry');
  });

  it('returns null for unknown commands', () => {
    expect(resolveCommand('nonexistent')).toBeNull();
    expect(resolveCommand('')).toBeNull();
  });
});

describe('slashCommands: matchCommands', () => {
  it('returns all commands for an empty prefix', () => {
    expect(matchCommands('')).toHaveLength(SLASH_COMMANDS.length);
  });

  it('filters by name prefix', () => {
    const matches = matchCommands('re');
    expect(matches.map((c) => c.name)).toContain('retry');
  });

  it('filters by alias prefix', () => {
    const matches = matchCommands('n');
    expect(matches.map((c) => c.name)).toContain('new');
  });

  it('returns empty for no match', () => {
    expect(matchCommands('xyz')).toHaveLength(0);
  });
});

describe('slashCommands: dispatchSlashCommand', () => {
  it('returns handled=false for unknown commands', () => {
    const result = dispatchSlashCommand({ name: 'xyz', args: '', raw: '/xyz' });
    expect(result.handled).toBe(false);
    expect(result.consumed).toBe(false);
  });

  it('dispatches /new to the newThread callback', () => {
    const newThread = vi.fn(() => ({ ok: true }));
    const result = dispatchSlashCommand(
      { name: 'new', args: '', raw: '/new' },
      { newThread },
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(result.consumed).toBe(true);
    expect(newThread).toHaveBeenCalled();
  });

  it('dispatches /stop to the cancel callback', () => {
    const cancel = vi.fn(() => true);
    const result = dispatchSlashCommand(
      { name: 'stop', args: '', raw: '/stop' },
      { cancel },
      { busy: true }
    );
    expect(result.handled).toBe(true);
    expect(result.consumed).toBe(true);
    expect(cancel).toHaveBeenCalled();
  });

  it('rejects /stop when nothing is running', () => {
    const cancel = vi.fn();
    const result = dispatchSlashCommand(
      { name: 'stop', args: '', raw: '/stop' },
      { cancel },
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(result.consumed).toBe(true);
    expect(result.message).toBeTruthy();
    expect(cancel).not.toHaveBeenCalled();
  });

  it('dispatches /retry to the retryLast callback', () => {
    const retryLast = vi.fn(() => ({ ok: true }));
    const result = dispatchSlashCommand(
      { name: 'retry', args: '', raw: '/retry' },
      { retryLast },
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(retryLast).toHaveBeenCalled();
  });

  it('dispatches /undo to the undoLast callback', () => {
    const undoLast = vi.fn(() => ({ ok: true }));
    const result = dispatchSlashCommand(
      { name: 'undo', args: '', raw: '/undo' },
      { undoLast },
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(undoLast).toHaveBeenCalled();
  });

  it('handles /help locally with a help message', () => {
    const result = dispatchSlashCommand(
      { name: 'help', args: '', raw: '/help' },
      {},
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(result.consumed).toBe(true);
    expect(result.message).toBeTruthy();
    expect(result.message).toContain('/new');
    expect(result.message).toContain('/retry');
    expect(result.message).toContain('/undo');
    expect(result.message).toContain('/stop');
    expect(result.message).toContain('/help');
  });

  it('works with aliases', () => {
    const newThread = vi.fn(() => ({ ok: true }));
    const result = dispatchSlashCommand(
      { name: 'n', args: '', raw: '/n' },
      { newThread },
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(newThread).toHaveBeenCalled();
  });

  it('returns not-available message when callback is missing', () => {
    const result = dispatchSlashCommand(
      { name: 'new', args: '', raw: '/new' },
      {}, // no callbacks
      { busy: false }
    );
    expect(result.handled).toBe(true);
    expect(result.consumed).toBe(true);
    expect(result.message).toBeTruthy();
  });
});

describe('slashCommands: command definitions', () => {
  it('all commands have a name, hint, and label', () => {
    for (const cmd of SLASH_COMMANDS) {
      expect(cmd.name).toBeTruthy();
      expect(typeof cmd.hint).toBe('function');
      expect(typeof cmd.label).toBe('function');
      expect(cmd.scope).toMatch(/^(local|action)$/);
    }
  });

  it('all action commands have an action name', () => {
    for (const cmd of SLASH_COMMANDS) {
      if (cmd.scope === 'action') {
        expect(cmd.action).toBeTruthy();
      }
    }
  });

  it('command names are unique', () => {
    const names = SLASH_COMMANDS.map((c) => c.name);
    expect(new Set(names).size).toBe(names.length);
  });

  it('aliases do not collide with command names', () => {
    const names = new Set(SLASH_COMMANDS.map((c) => c.name));
    for (const cmd of SLASH_COMMANDS) {
      for (const alias of cmd.aliases) {
        expect(names.has(alias)).toBe(false);
      }
    }
  });
});
