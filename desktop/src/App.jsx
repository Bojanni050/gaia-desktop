import React, { useCallback, useEffect, useState } from 'react';
import Sidebar from './shell/Sidebar';
import Conversation from './conversation/Conversation';
import SettingsPanel from './settings/SettingsPanel';
import LibraryPanel from './library/LibraryPanel';
import AboutPanel from './settings/AboutPanel';
import { serverApi, presenceApi } from './server/api';
import { useConversation } from './state/useConversation';
import { useServerStatus } from './state/useServerStatus';
import { L } from './lib/lexicon';

/**
 * The desktop shell  the web's grid, the web's calm. Presence here is the
 * orb's breath (quiet / listening / thinking), plus the health whisper when
 * Gaia's server is beyond reach. Never a status dashboard.
 */
export default function App() {
  const status = useServerStatus(serverApi);
  const [quiet, setQuietState] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [libraryOpen, setLibraryOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [lang, setLang] = useState(localStorage.getItem('gaia.lang') || 'nl');
  const conversation = useConversation(serverApi);

  useEffect(() => {
    presenceApi.get().catch(() => {});
  }, []);

  // Bumped on every 'conversation.history.changed' server event (pushed via
  // ServerLink::spawn_event_bridge, backed by gaia-api's SSE endpoint) so
  // HistorySection can refresh its already-loaded list live  e.g. gaia-web
  // just saved a conversation this desktop app should now be able to see.
  // Deliberately does not touch the active thread: switching what's on
  // screen out from under someone is not what "keep history in sync" means.
  const [historyVersion, setHistoryVersion] = useState(0);
  useEffect(() => {
    let unlisten;
    serverApi.onServerEvent((event) => {
      if (event?.topic === 'conversation.history.changed') {
        setHistoryVersion((v) => v + 1);
      }
    }).then((fn) => { unlisten = fn; });
    return () => { if (unlisten) unlisten(); };
  }, []);

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
        onOpenAbout={() => setAboutOpen(true)}
        historyVersion={historyVersion}
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
      
      {aboutOpen && (
        <AboutPanel onClose={() => setAboutOpen(false)} />
      )}
    </div>
  );
}
