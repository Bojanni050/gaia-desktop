import { describe, it, expect } from 'vitest';
import { withDesktopCommander, DESKTOP_COMMANDER_ID } from './desktopCommander';

describe('withDesktopCommander', () => {
  it('adds the default server when none exists', () => {
    const out = withDesktopCommander({});
    expect(out.mcp.servers).toHaveLength(1);
    expect(out.mcp.servers[0]).toMatchObject({ id: DESKTOP_COMMANDER_ID, command: 'npx', enabled: true });
    expect(out.desktop.serverId).toBe(DESKTOP_COMMANDER_ID);
  });

  it('adopts a hand-added Desktop Commander server by renaming its id', () => {
    const legacy = { id: 'mcp-1', enabled: true, command: 'npx', args: ['-y', '@wonderwhy-er/desktop-commander@latest'] };
    const out = withDesktopCommander({ mcp: { servers: [legacy] }, desktop: { enabled: true, serverId: 'desktop-commander' } });
    expect(out.mcp.servers).toHaveLength(1);
    expect(out.mcp.servers[0]).toMatchObject({ id: DESKTOP_COMMANDER_ID, command: 'npx' });
    expect(out.desktop.enabled).toBe(true);
  });

  it('leaves a correctly configured server and other servers untouched', () => {
    const dc = { id: DESKTOP_COMMANDER_ID, enabled: false, command: 'node', args: ['x'] };
    const other = { id: 'fs', enabled: true, command: 'npx', args: ['server-filesystem'] };
    const out = withDesktopCommander({ mcp: { servers: [other, dc] } });
    expect(out.mcp.servers).toEqual([other, dc]);
  });

  it('never adopts an unrelated server', () => {
    const other = { id: 'fs', enabled: true, command: 'npx', args: ['server-filesystem'] };
    const out = withDesktopCommander({ mcp: { servers: [other] } });
    expect(out.mcp.servers.map((s) => s.id)).toEqual([DESKTOP_COMMANDER_ID, 'fs']);
  });
});
