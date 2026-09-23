/**
 * Settings — this device's behaviour only: which Gaia Cloud server to reach,
 * notifications, quiet presence, and the local capability surfaces (audio,
 * capture). Nothing cognitive ever appears here.
 *
 * Saving gives calm feedback: the button keeps its width, settles into a
 * soft "saved" state and quietly returns — never a jump or a flash.
 */
import React, { useEffect, useState } from 'react';
import { settingsApi, serverApi, audioApi, captureApi, mcpApi } from '../server/api';
import { L } from '../lib/lexicon';

export default function SettingsPanel({ onClose, quiet, onQuietChange }) {
  const [settings, setSettings] = useState(null);
  const [audio, setAudio] = useState(null);
  const [captureSources, setCaptureSources] = useState([]);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState(null);
  const [saveState, setSaveState] = useState('idle'); // idle | saving | saved
  const [showToken, setShowToken] = useState(false);
  const [toolsState, setToolsState] = useState({ loading: null, result: null, error: null });

  useEffect(() => {
    let active = true;
    settingsApi.get().then((s) => active && setSettings(s)).catch(() => active && setSettings({}));
    audioApi.getStatus().then((a) => active && setAudio(a)).catch(() => {});
    captureApi.listSources().then((c) => active && setCaptureSources(c || [])).catch(() => {});
    return () => {
      active = false;
    };
  }, []);

  if (!settings) {
    return (
      <div className="settings-overlay">
        <div className="settings-panel">…</div>
      </div>
    );
  }

  const patch = (part) => setSettings((prev) => ({ ...prev, ...part }));

  const mcpServers = () => settings.mcp?.servers || [];
  const setMcpServers = (servers) => patch({ mcp: { ...settings.mcp, servers } });
  const patchMcpServer = (index, part) =>
    setMcpServers(
      mcpServers().map((server, i) => (i === index ? { ...server, ...part } : server)),
    );
  const addMcpServer = () =>
    setMcpServers([
      ...mcpServers(),
      { id: `mcp-${Date.now()}`, enabled: true, command: '', args: [] },
    ]);
  const removeMcpServer = (index) => setMcpServers(mcpServers().filter((_, i) => i !== index));

  const showTools = async (serverId) => {
    setToolsState({ loading: serverId, result: null, error: null });
    try {
      const result = await mcpApi.listTools(serverId);
      setToolsState({ loading: null, result, error: null });
    } catch (_) {
      setToolsState({ loading: null, result: null, error: serverId });
    }
  };

  const save = async () => {
    if (saveState === 'saving') return;
    setSaveState('saving');
    try {
      const saved = await settingsApi.save(settings);
      setSettings(saved);
      setSaveState('saved');
      setTimeout(() => setSaveState('idle'), 1800);
    } catch (_) {
      setSaveState('idle');
    }
  };

  const testConnection = async () => {
    setTesting(true);
    setTestResult(null);
    try {
      await serverApi.applyConfig(settings.server);
      const status = await serverApi.testConnection();
      setTestResult(status);
    } catch (error) {
      setTestResult('error');
    } finally {
      setTesting(false);
    }
  };

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.settingsTitle}</h2>

        <section>
          <h3>{L.settingsCloud}</h3>
          <label className="field">
            <span>{L.settingsServerUrl}</span>
            <input
              type="url"
              value={settings.server?.baseUrl || ''}
              placeholder="https://gaia.example/api"
              onChange={(e) => patch({ server: { ...settings.server, baseUrl: e.target.value || null } })}
            />
          </label>
          <label className="field">
            <span>{L.settingsAuthToken}</span>
            <div className="token-row">
              <input
                type={showToken ? 'text' : 'password'}
                value={settings.server?.authToken || ''}
                onChange={(e) => patch({ server: { ...settings.server, authToken: e.target.value || null } })}
              />
              <button
                type="button"
                className="token-toggle"
                onClick={() => setShowToken((prev) => !prev)}
              >
                {showToken ? '✓' : '○'}
              </button>
            </div>
          </label>
          <div className="field-row">
            <button onClick={testConnection} disabled={testing}>
              {testing ? L.settingsTesting : L.settingsTest}
            </button>
            {testResult && <span className={`test-result test-${testResult}`}>{String(testResult)}</span>}
          </div>
        </section>

        <section>
          <h3>{L.settingsBehaviour}</h3>
          <label className="field field-toggle">
            <input
              type="checkbox"
              checked={settings.notifications?.enabled ?? true}
              onChange={(e) => patch({ notifications: { enabled: e.target.checked } })}
            />
            <span>{L.settingsNotifications}</span>
          </label>
          <label className="field field-toggle">
            <input type="checkbox" checked={quiet} onChange={(e) => onQuietChange(e.target.checked)} />
            <span>{L.settingsQuiet}</span>
          </label>
        </section>

        <section>
          <h3>{L.settingsCapabilities}</h3>
          <p className="capability-line">
            {L.settingsMicrophone}: {audio ? audio.permission : '—'}
          </p>
          <p className="capability-line">
            {L.settingsCaptureSources}:{' '}
            {captureSources.length === 0
              ? L.settingsCaptureNone
              : captureSources.map((s) => s.name).join(', ')}
          </p>
        </section>

        <section>
          <h3>{L.settingsMcp}</h3>
          <p className="capability-line">{L.settingsMcpHint}</p>
          {(settings.mcp?.servers || []).length === 0 && (
            <p className="capability-line">{L.settingsMcpEmpty}</p>
          )}
          {(settings.mcp?.servers || []).map((server, index) => (
            <div className="mcp-server" key={server.id || index}>
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={server.enabled ?? true}
                  onChange={(e) => patchMcpServer(index, { enabled: e.target.checked })}
                />
                <span>{server.command ? `${server.id}` : L.settingsMcpCommand}</span>
              </label>
              <label className="field">
                <span>{L.settingsMcpCommand}</span>
                <input
                  type="text"
                  value={server.command || ''}
                  placeholder="npx"
                  onChange={(e) => patchMcpServer(index, { command: e.target.value })}
                />
              </label>
              <label className="field">
                <span>{L.settingsMcpArgs}</span>
                <input
                  type="text"
                  value={(server.args || []).join(' ')}
                  placeholder="-y @modelcontextprotocol/server-filesystem ~/Documents"
                  onChange={(e) =>
                    patchMcpServer(index, {
                      args: e.target.value.split(/\s+/).filter(Boolean),
                    })
                  }
                />
              </label>
              <div className="field-row">
                <button onClick={() => showTools(server.id)} disabled={!server.id || toolsState.loading === server.id}>
                  {toolsState.loading === server.id ? L.settingsMcpToolsLoading : L.settingsMcpTools}
                </button>
                {toolsState.result?.serverId === server.id && (
                  <span className="capability-line">
                    {toolsState.result.tools.map((t) => t.name).join(', ') || '—'}
                  </span>
                )}
                {toolsState.error === server.id && (
                  <span className="capability-line">{L.settingsMcpToolsFailed}</span>
                )}
                <button onClick={() => removeMcpServer(index)}>{L.settingsMcpRemove}</button>
              </div>
            </div>
          ))}
          <div className="field-row">
            <button onClick={addMcpServer}>{L.settingsMcpAdd}</button>
          </div>
        </section>

        <div className="settings-actions">
          <button
            className={`primary save-btn${saveState === 'saved' ? ' saved' : ''}`}
            onClick={save}
            disabled={saveState !== 'idle'}
          >
            <span className="save-label">
              {saveState === 'saving' ? L.settingsSaving : saveState === 'saved' ? L.settingsSaved : L.settingsSave}
            </span>
          </button>
          <button onClick={onClose}>{L.settingsClose}</button>
        </div>
      </div>
    </div>
  );
}
