import React, { useCallback, useEffect, useState } from 'react';
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

  // Auto-resume: on launch, reopen the most recently active conversation
  // (historyApi.list is already sorted newest-first by gaia-api) instead of
  // starting blank every time. Only "New page" mints a fresh thread id
  // after this — see Sidebar's onNew.
  useEffect(() => {
    let cancelled = false;
    historyApi.list().then(async (list) => {
      if (cancelled || list.length === 0) return;
      const [mostRecent] = list;
      const { messages } = await historyApi.get(mostRecent.id);
      if (cancelled) return;
      conversation.hydrateThread(mostRecent.id, messages);
    }).catch(() => { /* no history yet, or unreachable — start fresh */ });
    return () => { cancelled = true; };
  }, [conversation.hydrateThread]);

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
