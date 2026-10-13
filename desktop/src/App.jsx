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
import { serverApi, presenceApi, settingsApi, historyApi } from './server/api';
import { useConversation } from './state/useConversation';
import { useResearchJobs } from './state/useResearchJobs';
import { useServerStatus } from './state/useServerStatus';
import { useVoiceSession } from './state/useVoiceSession';
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
  // Hands-free: turns what Rust hears into conversation (and interruptions into cancelled turns).
  const voiceSession = useVoiceSession({ conversation });

  // Background research: the findings are written into the conversation by the
  // server, so delivery here is "re-read the thread", then mark the job seen.
  // A job for another conversation is left for the person to open — this must
  // never pull them out of the thread they are reading.
  //
  // The ack goes through a ref because the two are mutually dependent: the
  // callback needs the hook's ack, and the hook needs the callback.
  const researchAckRef = useRef(null);
  const handleResearchFinished = useCallback(
    async (job) => {
      if (job.conversationId && job.conversationId === conversation.activeId) {
        try {
          const { messages } = await historyApi.get(job.conversationId);
          conversation.hydrateThread(job.conversationId, messages);
        } catch (_) {
          return; // unread: the job stays unseen and is retried on the next poll
        }
      }
      if (researchAckRef.current) await researchAckRef.current(job.id);
    },
    [conversation.activeId, conversation.hydrateThread]
  );

  const research = useResearchJobs(serverApi, { onFinished: handleResearchFinished });
  researchAckRef.current = research.ack;

  // Opening a finished job from another thread: read that conversation, put it
  // on screen, then mark the job seen.
  const handleOpenResearchConversation = useCallback(
    async (job) => {
      try {
        const { messages } = await historyApi.get(job.conversationId);
        conversation.hydrateThread(job.conversationId, messages);
        await research.ack(job.id);
      } catch (_) {
        // Unreadable: leave it in the list rather than pretend it was opened.
      }
    },
    [conversation.hydrateThread, research.ack]
  );

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

  // The orb rests quiet by default; Gaia is present, not performative. While she hears the user speak
  // hands-free it listens with them.
  const presenceState = voiceSession.state === 'hearing' ? 'listening' : 'quiet';
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
          listenState={voiceSession.state}
          whisper={whisper}
          onSend={conversation.send}
          onRetry={conversation.retry}
          research={research}
          onOpenResearchConversation={handleOpenResearchConversation}
        />
      </main>

      {settingsOpen && (
        <SettingsPanel
          onClose={() => setSettingsOpen(false)}
          quiet={quiet}
          onQuietChange={handleQuiet}
          listenState={voiceSession.state}
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

      {/* Understanding — a right-hand drawer: closed, only a 16px edge with a
          golden dot that pulses while something unseen is waiting. */}
      <UnderstandingRail />
    </div>
  );
}
