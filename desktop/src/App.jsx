import React, { useCallback, useEffect, useRef, useState } from 'react';
import Sidebar from './shell/Sidebar';
import ConnectionNotice from './shell/ConnectionNotice';
import Conversation from './conversation/Conversation';
import SettingsPanel from './settings/SettingsPanel';
import LibraryPanel from './library/LibraryPanel';
import AboutPanel from './settings/AboutPanel';
import UpdatePanel from './settings/UpdatePanel';
import EpisodeTimeline from './logos/EpisodeTimeline';
import UnderstandingRail from './cognition/UnderstandingRail';
import { serverApi, presenceApi, settingsApi } from './server/api';import { useConversation } from './state/useConversation';
import { useServerStatus } from './state/useServerStatus';
import { setSpeechGain } from './lib/speech';
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
  const [updateOpen, setUpdateOpen] = useState(false);
  const [logosOpen, setLogosOpen] = useState(false);
  const [lang, setLang] = useState(localStorage.getItem('gaia.lang') || 'nl');
  const conversation = useConversation(serverApi);

  useEffect(() => {
    presenceApi.get().catch(() => {});
    // Gaia's voice plays inside the WebView2 process, so Windows shows no
    // per-app slider for it — load the saved gain here so her volume is
    // right from the first reply, before Settings is ever opened.
    settingsApi.get().then((s) => setSpeechGain(s?.audio || {})).catch(() => {});
    // Actively probe the link on launch so the startup offline pop
    // reflects a fresh check, not a stale cached status. The result
    // flows back through useServerStatus via the server://status event.
    serverApi.testConnection().catch(() => {});
  }, []);

  // Startup offline pop: latch the first resolved status after
  // 'connecting'. If it is anything but online, fade in a notice
  // asking the user to check credentials. Later transitions never
  // re-trigger it — this is strictly a launch check.
  const startupCheckedRef = useRef(false);
  const [startupOffline, setStartupOffline] = useState(false);
  const [noticeDismissed, setNoticeDismissed] = useState(false);
  useEffect(() => {
    if (status !== 'connecting' && !startupCheckedRef.current) {
      startupCheckedRef.current = true;
      if (status !== 'online') setStartupOffline(true);
    }
  }, [status]);

  const showConnectionNotice =
    startupOffline && !noticeDismissed && status !== 'online';

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
    status === 'offline'
      ? L.healthWhisper
      : status === 'unauthorized'
        ? L.turnUnauthorized
        : null;

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
        onOpenUpdates={() => setUpdateOpen(true)}
        onOpenLogos={() => setLogosOpen(true)}
        historyVersion={historyVersion}
      />

      <main className="gaia-main">
        <Conversation
          thread={conversation.active}
          busy={conversation.busy}
          streaming={conversation.streaming}
          progress={conversation.progress}
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

      {updateOpen && (
        <UpdatePanel onClose={() => setUpdateOpen(false)} />
      )}

      {/*
        The Kairos episode timeline reads live from Gaia Cloud
        (GET /kairos/episodes) — the derived narrative episodes the Kairos
        worker synthesises from raw observations, with each episode's raw
        evidence loaded on demand. No mock feed: the drawer shows its honest
        loading / empty / failed states against the real endpoint.
      */}
      {logosOpen && (
        <EpisodeTimeline onClose={() => setLogosOpen(false)} />
      )}

      <ConnectionNotice
        open={showConnectionNotice}
        status={status}
        onOpenSettings={() => {
          setNoticeDismissed(true);
          setSettingsOpen(true);
        }}
        onDismiss={() => setNoticeDismissed(true)}
      />

      {/* Understanding — a right-hand drawer: closed, only a 10px edge with a
          golden dot that pulses while something unseen is waiting. */}
      <UnderstandingRail />
    </div>
  );
}
