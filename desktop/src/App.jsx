import React, { useCallback, useEffect, useState } from 'react';
import Sidebar from './shell/Sidebar';
import Conversation from './conversation/Conversation';
import SettingsPanel from './settings/SettingsPanel';
import { serverApi, presenceApi } from './server/api';
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
  const [lang, setLang] = useState(localStorage.getItem('gaia.lang') || 'nl');
  const conversation = useConversation(serverApi);

  useEffect(() => {
    presenceApi.get().catch(() => {});
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
      />

      <main className="gaia-main">
        <Conversation
          thread={conversation.active}
          busy={conversation.busy}
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
    </div>
  );
}
