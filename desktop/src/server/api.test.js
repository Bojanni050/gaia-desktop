import { describe, it, expect, vi, beforeEach } from 'vitest';

const invoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args) => invoke(...args),
}));

import { mcpApi, speechApi, getSpeechInfo } from './api';

describe('mcpApi', () => {
  beforeEach(() => invoke.mockClear());

  it('lists tools through the mcp_list_tools command', async () => {
    invoke.mockResolvedValue({ serverId: 's1', tools: [] });
    await mcpApi.listTools('s1');
    expect(invoke).toHaveBeenCalledWith('mcp_list_tools', { serverId: 's1' });
  });

  it('calls a tool through mcp_call_tool, arguments passed verbatim', async () => {
    invoke.mockResolvedValue({ content: [] });
    await mcpApi.callTool('s1', 'echo', { message: 'hallo' });
    expect(invoke).toHaveBeenCalledWith('mcp_call_tool', {
      serverId: 's1',
      tool: 'echo',
      arguments: { message: 'hallo' },
    });
  });
});

describe('speechApi', () => {
  beforeEach(() => invoke.mockClear());

  it('returns bytes and the server mime type from the speech_synthesize command', async () => {
    invoke.mockResolvedValue({ audio: [73, 68, 51], mime_type: 'audio/mpeg' });
    const result = await speechApi.synthesize('hallo daar');
    expect(invoke).toHaveBeenCalledWith('speech_synthesize', { text: 'hallo daar' });
    expect(result.bytes).toEqual(new Uint8Array([73, 68, 51]));
    expect(result.mimeType).toBe('audio/mpeg');
  });

  it('still accepts a bare byte array from older shells, labeled wav', async () => {
    invoke.mockResolvedValue([82, 73, 70, 70]);
    const result = await speechApi.synthesize('hello there');
    expect(result.bytes).toEqual(new Uint8Array([82, 73, 70, 70]));
    expect(result.mimeType).toBe('audio/wav');
  });

  it('falls back to wav when the shell sends no mime type', async () => {
    invoke.mockResolvedValue({ audio: [1, 2, 3] });
    const result = await speechApi.synthesize('hi');
    expect(result.mimeType).toBe('audio/wav');
  });
});

describe('getSpeechInfo', () => {
  it('returns the voice description body from speech/info', async () => {
    const request = vi.fn(async () => ({
      body: { configured: true, provider: 'mistral', languages: ['en', 'nl'] },
    }));
    const info = await getSpeechInfo(request);
    expect(request).toHaveBeenCalledWith({ method: 'get', path: 'speech/info' });
    expect(info.languages).toContain('nl');
  });

  it('resolves undefined when the request fails — callers keep the legacy gate', async () => {
    await expect(getSpeechInfo(async () => { throw new Error('gone'); })).resolves.toBeUndefined();
    await expect(getSpeechInfo(undefined)).resolves.toBeUndefined();
    await expect(getSpeechInfo(null)).resolves.toBeUndefined();
  });
});
