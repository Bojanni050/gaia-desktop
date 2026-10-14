import { useEffect, useMemo, useSyncExternalStore } from 'react';
import { serverApi, settingsApi, mcpApi, desktopApi } from '../server/api';
import { createDesktopActions } from './desktopActions';

/**
 * The window's desktop-action controller (state/desktopActions.js) plus what it
 * currently shows: `prompts` (actions waiting for the person) and `notice`.
 * Closing the window while a prompt is open counts as no answer — the server
 * lets the action expire when the stream goes away.
 */
export function useDesktopActions() {
  const controller = useMemo(
    () =>
      createDesktopActions({
        request: serverApi.request,
        callTool: mcpApi.callTool,
        listTools: mcpApi.listTools,
        getSettings: settingsApi.get,
        getPlaces: desktopApi.knownPlaces,
      }),
    []
  );
  const snapshot = useSyncExternalStore(controller.subscribe, controller.getSnapshot);
  useEffect(() => () => controller.abortAll(), [controller]);
  return { controller, prompts: snapshot.prompts, notice: snapshot.notice };
}
