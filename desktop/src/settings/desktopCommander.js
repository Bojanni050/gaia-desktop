// Desktop Commander is the one MCP server Gaia's "work on this computer" capability
// relies on, so it always exists under a fixed id and has its own tab in settings.

export const DESKTOP_COMMANDER_ID = 'desktop-commander';

const DEFAULT_SERVER = {
  id: DESKTOP_COMMANDER_ID,
  name: '',
  enabled: true,
  command: 'npx',
  args: ['-y', '@wonderwhy-er/desktop-commander@latest'],
};

const looksLikeDesktopCommander = (server) =>
  (server.args || []).some((arg) => /desktop-commander/i.test(arg));

/**
 * Returns settings in which the Desktop Commander server exists under its fixed id and the
 * desktop capability points at it. A server that already runs Desktop Commander under another id
 * (as one added by hand gets `mcp-<timestamp>`) is adopted by renaming its id, so the
 * capability's lookup by `desktop.serverId` finds it; otherwise the default entry is added.
 * Nothing is persisted until the user saves.
 */
export function withDesktopCommander(settings) {
  const servers = settings.mcp?.servers || [];
  let next = servers;
  if (!servers.some((s) => s.id === DESKTOP_COMMANDER_ID)) {
    const legacy = servers.find(looksLikeDesktopCommander);
    next = legacy
      ? servers.map((s) => (s === legacy ? { ...s, id: DESKTOP_COMMANDER_ID } : s))
      : [{ ...DEFAULT_SERVER }, ...servers];
  }
  return {
    ...settings,
    mcp: { ...settings.mcp, servers: next },
    desktop: { ...settings.desktop, serverId: DESKTOP_COMMANDER_ID },
  };
}
