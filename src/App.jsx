import React, { useEffect, useState } from 'react';
import Sidebar from './shell/Sidebar';
import PresenceBar from './presence/PresenceBar';
import Conversation from './conversation/Conversation';
import Composer from './conversation/Composer';
import SettingsPanel from './settings/SettingsPanel';
import { serverApi, presenceApi } from './server/api';
import { useConversation } from './state/useConversation';
import { useServerStatus } from './state/useServerStatus';
import { mapConnectionToPresence } from './presence/PresenceOrb';

export default function App() {
  const status = useServerStatus(serverApi);
  const [quiet, setQuietState] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [serverUrl, setServerUrl] = useState(null);
  const conversation = useConversation(serverApi);

  useEffect(() => {
    let active = true;
    presenceApi.get().then(() => {}).catch(() => {});
    serverApi
      .getStatus()
      .then((payload) => {
        if (active) setServerUrl(payload?.serverUrl || null);
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  const presence = quiet ? 'quiet' : mapConnectionToPresence(status);

  const handleQuiet = (next) => {
    setQuietState(next);
    presenceApi.setQuiet(next).catch(() => {});
  };

  return (
    <div className="shell">
      <Sidebar
        threads={conversation.threads}
        activeId={conversation.activeId}
        presence={presence}
        onSelect={conversation.openThread}
        onNew={conversation.newThread}
        onOpenSettings={() => setSettingsOpen(true)}
        onToggleQuiet={() => handleQuiet(!quiet)}
      />
      <main className="main">
        <PresenceBar presence={presence} serverUrl={serverUrl} />
        <Conversation thread={conversation.active} busy={conversation.busy} presence={presence} />
        <Composer onSend={conversation.send} busy={conversation.busy} />
      </main>
      {settingsOpen && (
        <SettingsPanel onClose={() => setSettingsOpen(false)} quiet={quiet} onQuietChange={handleQuiet} />
      )}
    </div>
  );
}
