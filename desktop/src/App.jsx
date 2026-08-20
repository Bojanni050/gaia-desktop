import React, { useCallback, useEffect, useRef, useState } from 'react';
import Sidebar from './shell/Sidebar';
import Conversation from './conversation/Conversation';
import SettingsPanel from './settings/SettingsPanel';
import LibraryPanel from './library/LibraryPanel';
import { serverApi, presenceApi, historyApi } from './server/api';
import { useConversation } from './state/useConversation';
import { useServerStatus } from './state/useServerStatus';
import { L } from './lib/lexicon';

/**
 * The desktop shell — the web's grid, the web's calm. Presence here is the
 * orb's breath (quiet / listening / thinking), plus the health whisper when
 * Gaia's server is beyond reach. Never a status dashboard.
 */
export default function App() {
  const status = useServerStatus(serverApi);
  const [quiet, setQuietState] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [libraryOpen, setLibraryOpen] = useState(false);
  const [lang, setLang] = useState(localStorage.getItem('gaia.lang') || 'nl');
  const conversation = useConversation(serverApi);

  useEffect(() => {
    presenceApi.get().catch(() => {});
  }, []);

  // Refs so the poll below can read current state without re-creating its
  // interval on every message/busy tick.
  const activeRef = useRef(null);
  useEffect(() => { activeRef.current = conversation.active; }, [conversation.active]);
  const busyRef = useRef(false);
  useEffect(() => { busyRef.current = conversation.busy; }, [conversation.busy]);

  /**
   * Resumes whichever conversation gaia-api considers most recently
   * updated (historyApi.list is newest-first) if it differs from what's
   * open here. Called once on launch and then on a poll interval, so
   * switching between gaia-desktop and gaia-web picks up whatever the
   * *other* client most recently did — there is no push channel between
   * clients, so this poll is the whole "shared session" mechanism for now.
   *
   * Two guards keep it from clobbering local, not-yet-saved state:
   *   - a turn in flight here is never interrupted
   *   - a thread just started via "New page" (no messages sent yet, so it
   *     isn't in gaia-api's list at all) is left alone rather than being
   *     silently replaced by whatever another client last touched
   */
  const syncMostRecentConversation = useCallback(async () => {
    if (busyRef.current) return;
    const list = await historyApi.list();
    if (list.length === 0) return;
    const [mostRecent] = list;
    const current = activeRef.current;
    if (current && current.id === mostRecent.id) return;
    if (current && current.messages.length === 0) return;

    const { messages } = await historyApi.get(mostRecent.id);
    conversation.hydrateThread(mostRecent.id, messages);
  }, [conversation.hydrateThread]);

  useEffect(() => {
    let cancelled = false;
    syncMostRecentConversation().catch(() => { /* no history yet, or unreachable — start fresh */ });
    const interval = setInterval(() => {
      if (!cancelled) syncMostRecentConversation().catch(() => {});
    }, 20000);
    return () => { cancelled = true; clearInterval(interval); };
  }, [syncMostRecentConversation]);

  const handleLangChange = useCallback((next) => {
    localStorage.setItem('gaia.lang', next);
    setLang(next);
  }, []);

  const handleQuiet = useCallback((next) => {
    setQuietState(next);
    presenceApi.setQuiet(next).catch(() => {});
  }, []);

  // The orb rests quiet by default; Gaia is present, not performative.
  const presenceState = 'quiet';
  const whisper =
    status === 'offline' ? L.healthWhisper : null;

  return (
    <div className="gaia-shell">
      <Sidebar
        threads={conversation.threads}
        activeId={conversation.activeId}
        lang={lang}
        onSelect={conversation.openThread}
        onNew={conversation.newThread}
        onDelete={conversation.deleteThread}
        onLangChange={handleLangChange}
        onOpenSettings={() => setSettingsOpen(true)}
        onOpenLibrary={() => setLibraryOpen(true)}
        onOpenHistoryConversation={conversation.hydrateThread}
      />

      <main className="gaia-main">
        <Conversation
          thread={conversation.active}
          busy={conversation.busy}
          streaming={conversation.streaming}
          presenceState={presenceState}
          whisper={whisper}
          onSend={conversation.send}
          onRetry={conversation.retry}
        />
      </main>

      {settingsOpen && (
        <SettingsPanel
          onClose={() => setSettingsOpen(false)}
          quiet={quiet}
          onQuietChange={handleQuiet}
        />
      )}

      {libraryOpen && <LibraryPanel onClose={() => setLibraryOpen(false)} />}
    </div>
  );
}
