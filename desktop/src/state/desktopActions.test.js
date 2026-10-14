import { describe, it, expect, vi } from 'vitest';
import {
  parseDesktopFrame,
  normalizePath,
  isWithinAllowed,
  blockedCommandHit,
  checkLimits,
  toolCallsFor,
  readToolResult,
  truncateOutput,
  createDesktopActions,
  MAX_OUTPUT_CHARS,
} from './desktopActions';
import { buildStreamTurnBody } from './contract';

const DESKTOP = {
  enabled: true,
  serverId: 'dc',
  allowedDirs: ['C:\\Users\\bo\\Documents'],
  blockedCommands: ['rm -rf', 'format '],
};

const frame = (fields) => ({
  id: 'a1',
  skill: 'run-command',
  policy: 'confirm',
  status: 'awaiting_confirmation',
  summary: 'Run command: npm test',
  input: { command: 'npm test' },
  conversationId: 'c1',
  ...fields,
});

function setup({ settings = DESKTOP, callTool, listTools } = {}) {
  const request = vi.fn(async () => ({ status: 204 }));
  const tool = callTool || vi.fn(async () => ({ content: [{ type: 'text', text: 'ok-output' }] }));
  const controller = createDesktopActions({
    request,
    callTool: tool,
    listTools: listTools || vi.fn(async () => []),
    getSettings: async () => ({ desktop: settings }),
  });
  const posts = () => request.mock.calls.map(([r]) => [r.path, r.body]);
  return { controller, request, tool, posts };
}

describe('the turn body', () => {
  it('declares desktop only when asked to, and never as false', () => {
    const messages = [{ role: 'user', content: 'hoi' }];
    expect(buildStreamTurnBody(messages, 't1', { desktop: true }).desktop).toBe(true);
    expect('desktop' in buildStreamTurnBody(messages, 't1', { desktop: false })).toBe(false);
    expect('desktop' in buildStreamTurnBody(messages, 't1')).toBe(false);
  });
});

describe('frames', () => {
  it('keeps what the client needs and fails closed on an unclear policy', () => {
    expect(parseDesktopFrame(frame({ policy: 'whatever' })).policy).toBe('confirm');
    expect(parseDesktopFrame(frame({ policy: 'auto' })).policy).toBe('auto');
    expect(parseDesktopFrame(null)).toBeNull();
    expect(parseDesktopFrame({ skill: 'x', status: 'approved' })).toBeNull();
  });
});

describe('the person\u2019s limits', () => {
  it('resolves paths before comparing them', () => {
    expect(normalizePath('C:\\Users\\Bo\\Documents\\..\\Documents\\a.txt')).toBe('c:/users/bo/documents/a.txt');
    expect(normalizePath('relative/path')).toBeNull();
    expect(normalizePath('~/x')).toBeNull();
    expect(normalizePath('/a/../..')).toBeNull();
  });

  it('allows the folder itself and what is inside it, case-insensitively on Windows', () => {
    const dirs = DESKTOP.allowedDirs;
    expect(isWithinAllowed('C:\\Users\\bo\\Documents', dirs)).toBe(true);
    expect(isWithinAllowed('c:/users/BO/documents/sub/a.txt', dirs)).toBe(true);
  });

  it('refuses outside the folders, including look-alikes and ..-escapes', () => {
    const dirs = DESKTOP.allowedDirs;
    expect(isWithinAllowed('C:\\Users\\bo\\Documents2\\a.txt', dirs)).toBe(false);
    expect(isWithinAllowed('C:\\Users\\bo\\Documents\\..\\.ssh\\id_rsa', dirs)).toBe(false);
    expect(isWithinAllowed('D:\\Documents\\a.txt', dirs)).toBe(false);
    expect(isWithinAllowed('C:\\Users\\bo\\Documents\\a.txt', [])).toBe(false);
    expect(isWithinAllowed('C:\\Users\\bo\\Documents\\a.txt', [''])).toBe(false);
  });

  it('matches the blocklist regardless of case and spacing', () => {
    expect(blockedCommandHit('RM   -RF /tmp', DESKTOP.blockedCommands)).toBe('rm -rf');
    expect(blockedCommandHit('npm test', DESKTOP.blockedCommands)).toBeNull();
    expect(blockedCommandHit('anything', ['', '  '])).toBeNull();
  });

  it('gives a reason for every refusal', () => {
    const outside = checkLimits('file-read', { path: 'C:\\Windows\\system.ini' }, DESKTOP);
    expect(outside.ok).toBe(false);
    expect(outside.reason).toMatch(/toegestane mappen/);
    expect(checkLimits('run-command', { command: 'rm -rf x' }, DESKTOP).ok).toBe(false);
    expect(checkLimits('run-command', { command: '' }, DESKTOP).ok).toBe(false);
    expect(checkLimits('delete-all', {}, DESKTOP).ok).toBe(false);
    expect(checkLimits('file-read', { path: 'C:\\Users\\bo\\Documents\\a.txt' }, DESKTOP).ok).toBe(true);
    expect(checkLimits('run-command', { command: 'npm test' }, DESKTOP).ok).toBe(true);
  });
});

describe('mapping skills onto Desktop Commander', () => {
  it('maps the four skills and nothing else', () => {
    expect(toolCallsFor('file-read', { path: 'p' })[0]).toEqual({ tool: 'read_file', args: { path: 'p' } });
    expect(toolCallsFor('file-write', { path: 'p', content: 'x' })[0].tool).toBe('write_file');
    expect(toolCallsFor('run-command', { command: 'ls' })[0].tool).toBe('start_process');
    expect(toolCallsFor('file-search', { path: 'p', query: 'q' }).map((c) => c.args.searchType)).toEqual([
      'files',
      'content',
    ]);
    expect(toolCallsFor('something-else', {})).toEqual([]);
  });

  it('reads the text and the error flag of a tool result, and clips output', () => {
    expect(readToolResult({ content: [{ type: 'text', text: 'a' }, { type: 'image' }, { type: 'text', text: 'b' }] })).toEqual({
      text: 'a\nb',
      isError: false,
    });
    expect(readToolResult({ content: [], isError: true }).isError).toBe(true);
    expect(truncateOutput('x'.repeat(MAX_OUTPUT_CHARS + 50)).length).toBe(MAX_OUTPUT_CHARS + 1);
  });
});

describe('a read runs without a prompt', () => {
  it('executes an auto action once it is approved and reports the output', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(
      frame({ id: 'r1', skill: 'file-read', policy: 'auto', status: 'approved', input: { path: 'C:\\Users\\bo\\Documents\\a.txt' } })
    );
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(tool).toHaveBeenCalledWith('dc', 'read_file', { path: 'C:\\Users\\bo\\Documents\\a.txt' });
    expect(posts()).toEqual([['desktop/actions/r1/result', { ok: true, output: 'ok-output' }]]);
  });

  it('refuses a read outside the allowed folders and says why', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(
      frame({ id: 'r2', skill: 'file-read', policy: 'auto', status: 'approved', input: { path: 'C:\\Windows\\system.ini' } })
    );
    expect(tool).not.toHaveBeenCalled();
    const [[path, body]] = posts();
    expect(path).toBe('desktop/actions/r2/result');
    expect(body.ok).toBe(false);
    expect(body.output).toMatch(/buiten de toegestane mappen/);
  });
});

describe('a write or command asks first', () => {
  it('shows a prompt with the full input and runs nothing yet', async () => {
    const { controller, tool, request } = setup();
    await controller.handleFrame(frame());
    const [prompt] = controller.getSnapshot().prompts;
    expect(prompt.summary).toBe('Run command: npm test');
    expect(prompt.input).toEqual({ command: 'npm test' });
    expect(tool).not.toHaveBeenCalled();
    expect(request).not.toHaveBeenCalled();
  });

  it('runs after approval, and reports the result', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(tool).not.toHaveBeenCalled(); // approval alone runs nothing; the server's `approved` does
    await controller.handleFrame(frame({ status: 'approved' }));
    expect(tool).toHaveBeenCalledWith('dc', 'start_process', { command: 'npm test', timeout_ms: 15000 });
    expect(posts()).toEqual([
      ['desktop/actions/a1/decision', { approved: true }],
      ['desktop/actions/a1/result', { ok: true, output: 'ok-output' }],
    ]);
  });

  it('does not run after a refusal, and Gaia is told so', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(frame());
    await controller.decide('a1', false);
    await controller.handleFrame(frame({ status: 'denied' }));
    expect(tool).not.toHaveBeenCalled();
    expect(posts()).toEqual([['desktop/actions/a1/decision', { approved: false }]]);
  });

  it('never runs a confirm action the person did not approve here, even if the frame says approved', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(frame({ status: 'approved' }));
    expect(tool).not.toHaveBeenCalled();
    const [[, body]] = posts();
    expect(body.ok).toBe(false);
  });

  it('refuses a blocked command without a prompt, by denying it', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(frame({ input: { command: 'rm -rf /' } }));
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(controller.getSnapshot().notice).toMatch(/blokkadelijst/);
    expect(tool).not.toHaveBeenCalled();
    expect(posts()).toEqual([['desktop/actions/a1/decision', { approved: false }]]);
  });

  it('refuses a write outside the allowed folders without a prompt', async () => {
    const { controller, posts } = setup();
    await controller.handleFrame(
      frame({ skill: 'file-write', input: { path: 'C:\\Windows\\x.txt', content: 'hi' } })
    );
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(posts()).toEqual([['desktop/actions/a1/decision', { approved: false }]]);
  });

  it('treats an unknown skill as nothing to run', async () => {
    const { controller, tool, posts } = setup();
    await controller.handleFrame(frame({ id: 'u1', skill: 'teleport', policy: 'auto', status: 'approved', input: {} }));
    expect(tool).not.toHaveBeenCalled();
    expect(posts()[0][1].ok).toBe(false);
  });
});

describe('several actions, repeats and endings', () => {
  it('keeps one prompt per id and ignores a repeated frame', async () => {
    const { controller } = setup();
    await controller.handleFrame(frame({ id: 'a' }));
    await controller.handleFrame(frame({ id: 'b', input: { command: 'ls' }, summary: 'Run command: ls' }));
    await controller.handleFrame(frame({ id: 'a' }));
    expect(controller.getSnapshot().prompts.map((p) => p.id)).toEqual(['a', 'b']);
  });

  it('runs an approved action only once when the frame repeats', async () => {
    const { controller, tool } = setup();
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    await controller.handleFrame(frame({ status: 'approved' }));
    await controller.handleFrame(frame({ status: 'approved' }));
    expect(tool).toHaveBeenCalledTimes(1);
  });

  it('answers a prompt only once', async () => {
    const { controller, request } = setup();
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    await controller.decide('a1', false);
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('clears the prompt when the server says it expired', async () => {
    const { controller, request } = setup();
    await controller.handleFrame(frame());
    await controller.handleFrame(frame({ status: 'expired' }));
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(request).not.toHaveBeenCalled();
  });

  it('clears every prompt when the turn is interrupted', async () => {
    const { controller } = setup();
    await controller.handleFrame(frame({ id: 'a' }));
    await controller.handleFrame(frame({ id: 'b' }));
    controller.abortAll();
    expect(controller.getSnapshot().prompts).toEqual([]);
  });

  it('does not report the result of an action that was interrupted while running', async () => {
    let finish;
    const callTool = vi.fn(() => new Promise((resolve) => { finish = resolve; }));
    const { controller, posts } = setup({ callTool });
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    const running = controller.handleFrame(frame({ status: 'approved' }));
    await vi.waitFor(() => expect(callTool).toHaveBeenCalled());
    controller.abortAll();
    finish({ content: [{ type: 'text', text: 'late' }] });
    await running;
    expect(posts().filter(([path]) => path.endsWith('/result'))).toEqual([]);
  });

  it('reports a failure with a calm notice when Desktop Commander drops away mid-turn', async () => {
    const callTool = vi.fn(async () => {
      throw new Error('spawn failed');
    });
    const { controller, posts } = setup({ callTool });
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    await controller.handleFrame(frame({ status: 'approved' }));
    const result = posts().find(([path]) => path.endsWith('/result'));
    expect(result[1].ok).toBe(false);
    expect(controller.getSnapshot().notice).toMatch(/Desktop Commander/);
  });

  it('turns a tool-reported error into ok:false', async () => {
    const callTool = vi.fn(async () => ({ content: [{ type: 'text', text: 'nope' }], isError: true }));
    const { controller, posts } = setup({ callTool });
    await controller.handleFrame(frame({ id: 'r', skill: 'file-read', policy: 'auto', status: 'approved', input: { path: 'C:\\Users\\bo\\Documents\\x' } }));
    expect(posts()[0][1]).toEqual({ ok: false, output: 'nope' });
  });

  it('clips large output before sending it', async () => {
    const callTool = vi.fn(async () => ({ content: [{ type: 'text', text: 'x'.repeat(50000) }] }));
    const { controller, posts } = setup({ callTool });
    await controller.handleFrame(frame({ id: 'r', skill: 'file-read', policy: 'auto', status: 'approved', input: { path: 'C:\\Users\\bo\\Documents\\x' } }));
    expect(posts()[0][1].output.length).toBe(MAX_OUTPUT_CHARS + 1);
  });

  it('shows nothing and sends nothing if the answer is rejected as already gone (404/409)', async () => {
    const request = vi.fn(async () => {
      throw { kind: 'communication', message: 'communication error: server responded with status 409' };
    });
    const controller = createDesktopActions({
      request,
      callTool: vi.fn(),
      listTools: vi.fn(),
      getSettings: async () => ({ desktop: DESKTOP }),
    });
    await controller.handleFrame(frame());
    await controller.decide('a1', true);
    expect(controller.getSnapshot().prompts).toEqual([]);
    expect(controller.getSnapshot().notice).toBeNull();
  });
});

describe('whether a turn may offer desktop', () => {
  it('is false when switched off, without folders, or when Desktop Commander does not answer', async () => {
    expect(await setup({ settings: { ...DESKTOP, enabled: false } }).controller.isAvailable()).toBe(false);
    expect(await setup({ settings: { ...DESKTOP, allowedDirs: [] } }).controller.isAvailable()).toBe(false);
    expect(await setup({ settings: { ...DESKTOP, allowedDirs: ['  '] } }).controller.isAvailable()).toBe(false);
    const down = setup({ listTools: vi.fn(async () => { throw new Error('no'); }) });
    expect(await down.controller.isAvailable()).toBe(false);
  });

  it('is true when allowed and running, and does not start the server again straight away', async () => {
    const listTools = vi.fn(async () => []);
    const { controller } = setup({ listTools });
    expect(await controller.isAvailable()).toBe(true);
    expect(await controller.isAvailable()).toBe(true);
    expect(listTools).toHaveBeenCalledTimes(1);
  });
});
