import { describe, it, expect, vi, beforeEach } from 'vitest';

const invoke = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args) => invoke(...args),
}));

import { mcpApi } from './api';

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
