/**
 * Settings — this device's behaviour only: which Gaia Cloud server to reach,
 * notifications, quiet presence, and the local capability surfaces (audio,
 * capture). Nothing cognitive ever appears here.
 *
 * Saving gives calm feedback: the button keeps its width, settles into a
 * soft "saved" state and quietly returns — never a jump or a flash.
 */
import React, { useEffect, useState } from 'react';
import { settingsApi, serverApi, audioApi, captureApi, mcpApi, voiceApi, speechApi } from '../server/api';
import { DESKTOP_COMMANDER_ID, withDesktopCommander } from './desktopCommander';
import { setSpeechGain } from '../lib/speech';
import { useVoiceStatus } from '../state/useVoiceStatus';
import { useWakeWord } from '../state/useWakeWord';
import WakeEnrollment from './WakeEnrollment';
import { L } from '../lib/lexicon';

/** The one sentence (if any) explaining the microphone's current problem or notice. */
function micMessage(voice) {
  const error = voice.status && voice.status.error;
  if (error) {
    return (
      {
        no_frames: L.settingsMicErrNoFrames,
        permission_denied: L.settingsMicErrDenied,
        no_device: L.settingsMicErrNoDevice,
        too_many_restarts: L.settingsMicErrRestarts,
      }[error.code] || L.settingsMicErrOpen
    );
  }
  const notice = voice.notice;
  if (notice && voice.status && voice.status.state === 'listening') {
    return (
      {
        device_fallback: L.settingsMicNoticeFallback,
        device_changed: L.settingsMicNoticeChanged,
      }[notice.code] || null
    );
  }
  return null;
}

export default function SettingsPanel({ onClose, quiet, onQuietChange, listenState = 'off' }) {
  const [settings, setSettings] = useState(null);
  const [audio, setAudio] = useState(null);
  const voice = useVoiceStatus();
  const [outputDevices, setOutputDevices] = useState([]);
  const refreshOutputs = () =>
    speechApi.listOutputDevices().then((d) => setOutputDevices(d || [])).catch(() => {});
  useEffect(() => {
    refreshOutputs();
  }, []);
  const wake = useWakeWord();
  const [enrolling, setEnrolling] = useState(false);
  const [sensitivity, setSensitivity] = useState(null); // while the slider is being dragged
  const [captureSources, setCaptureSources] = useState([]);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState(null);
  const [saveState, setSaveState] = useState('idle'); // idle | saving | saved
  const [showToken, setShowToken] = useState(false);
  const [toolsState, setToolsState] = useState({ loading: null, result: null, error: null });
  const [testState, setTestState] = useState({ loading: null, serverId: null, result: null, error: false });
  const [tab, setTab] = useState('general'); // general | tools
  const [mcpSel, setMcpSel] = useState(null); // selected MCP server id; null = the first one

  useEffect(() => {
    let active = true;
    settingsApi.get().then((s) => active && setSettings(withDesktopCommander(s))).catch(() => active && setSettings(withDesktopCommander({})));
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

  /** The "Hey Gaia" block: toggle, sensitivity, and enrolment. */
  function renderWake() {
    const w = wake.status;
    return (
      <div className="wake-block">
        <h4 className="wake-title">{L.wakeSection}</h4>
        <p className="capability-line">{L.wakeHint}</p>
        <p className="capability-line">{w && w.enrolled ? L.wakeEnrolled : L.wakeNotEnrolled}</p>
        <label className="field field-toggle">
          <input
            type="checkbox"
            checked={Boolean(w && w.enabled)}
            disabled={!w || !w.enrolled}
            onChange={async (e) => {
              const on = e.target.checked;
              patch({ voice: { ...settings.voice, wakeWordEnabled: on } });
              if (await wake.setEnabled(on) && on && !(voice.status && voice.status.micEnabled)) {
                await voice.setEnabled(true);
              }
            }}
          />
          <span>{L.wakeToggle}</span>
        </label>
        {w && w.enrolled && (
          <label className="field">
            <span>{L.wakeSensitivity}</span>
            <div className="volume-row">
              <span className="capability-line">{L.wakeMoreSensitive}</span>
              <input
                type="range"
                min={w.minSensitivity}
                max={w.maxSensitivity}
                step="0.01"
                value={sensitivity ?? w.sensitivity}
                onChange={(e) => setSensitivity(Number(e.target.value))}
                onMouseUp={() => sensitivity !== null && wake.setSensitivity(sensitivity).then(() => setSensitivity(null))}
                onKeyUp={() => sensitivity !== null && wake.setSensitivity(sensitivity).then(() => setSensitivity(null))}
                onBlur={() => sensitivity !== null && wake.setSensitivity(sensitivity).then(() => setSensitivity(null))}
              />
              <span className="capability-line">{L.wakeStricter}</span>
            </div>
          </label>
        )}
        {w && w.enrolled && <p className="capability-line">{L.wakeSensitivityHint}</p>}
        {listenState === 'waiting' && <p className="capability-line mic-note">{L.wakeWaiting}</p>}
        {enrolling ? (
          <WakeEnrollment
            onClose={() => setEnrolling(false)}
            onDone={(status) => {
              wake.enrolled(status);
              patch({ voice: { ...settings.voice, wakeEnrolled: true, wakeSensitivity: status.sensitivity } });
            }}
          />
        ) : (
          <div className="field-row">
            <button type="button" onClick={() => setEnrolling(true)}>
              {w && w.enrolled ? L.wakeReEnrol : L.wakeEnrol}
            </button>
            {w && w.enrolled && (
              <button
                type="button"
                onClick={async () => {
                  if (await wake.forget()) patch({ voice: { ...settings.voice, wakeEnrolled: false, wakeWordEnabled: false } });
                }}
              >
                {L.wakeForget}
              </button>
            )}
          </div>
        )}
      </div>
    );
  }

  // Hands-free: the preference is saved at once; switching it on also switches the microphone on, which
  // the user just asked for. (The microphone switch itself is never remembered across launches.)
  const setContinuous = async (checked) => {
    patch({ voice: { ...settings.voice, continuousEnabled: checked } });
    try {
      await voiceApi.continuousSet(checked);
    } catch (_) {
      /* the shell is unreachable: the checkbox simply does not take effect */
    }
    if (checked && !(voice.status && voice.status.micEnabled)) await voice.setEnabled(true);
  };
  const setBargeIn = async (checked) => {
    patch({ voice: { ...settings.voice, bargeInEnabled: checked } });
    try {
      await voiceApi.setBargeIn(checked);
    } catch (_) {
      /* ignored: the next successful change wins */
    }
  };
  // The volume gate for interrupting her: the slider moves the shown value at once and the shell hears
  // it when the drag ends (not on every step).
  const bargeInLevel = settings.voice?.bargeInLevel ?? 0.04;
  const commitBargeInLevel = async () => {
    try {
      await voiceApi.setBargeInLevel(bargeInLevel);
    } catch (_) {
      /* ignored: the next successful change wins */
    }
  };

  const mcpServers = () => settings.mcp?.servers || [];
  const setMcpServers = (servers) => patch({ mcp: { ...settings.mcp, servers } });
  const patchMcpServer = (index, part) =>
    setMcpServers(
      mcpServers().map((server, i) => (i === index ? { ...server, ...part } : server)),
    );
  const addMcpServer = () => {
    const id = `mcp-${Date.now()}`;
    setMcpServers([...mcpServers(), { id, enabled: true, command: '', args: [] }]);
    setMcpSel(id);
  };
  const removeMcpServer = (index) => {
    setMcpServers(mcpServers().filter((_, i) => i !== index));
    setMcpSel(null);
  };
  // Desktop Commander always exists and is the default tab; an unknown or removed
  // selection falls back to it.
  const dcId = DESKTOP_COMMANDER_ID;
  const dcIndex = mcpServers().findIndex((s) => s.id === dcId);
  const foundSel = mcpServers().findIndex((s) => s.id === mcpSel);
  const mcpSelectedIndex = foundSel >= 0 ? foundSel : dcIndex;
  const isDcSelected = mcpSelectedIndex === dcIndex;

  const desktopSettings = () => settings.desktop || {};
  const patchDesktop = (part) => patch({ desktop: { ...desktopSettings(), ...part } });
  // One entry per line, kept as typed (a trailing newline must survive while editing). Blank
  // entries never match anything, so an empty row cannot mean "allow everything".
  const lines = (text) => text.split('\n');

  const showTools = async (serverId) => {
    setToolsState({ loading: serverId, result: null, error: null });
    try {
      const result = await mcpApi.listTools(serverId);
      setToolsState({ loading: null, result, error: null });
    } catch (_) {
      setToolsState({ loading: null, result: null, error: serverId });
    }
  };

  // Tests the form as typed (unsaved). On success the server's own name fills an empty name
  // field; a name the user typed themselves is never overwritten.
  const testServer = async (index) => {
    const server = mcpServers()[index];
    setTestState({ loading: server.id, serverId: server.id, result: null, error: false });
    try {
      const result = await mcpApi.testServer(server.command || '', server.args || []);
      if (result.name && !(server.name || '').trim()) patchMcpServer(index, { name: result.name });
      setTestState({ loading: null, serverId: server.id, result, error: false });
    } catch (_) {
      setTestState({ loading: null, serverId: server.id, result: null, error: true });
    }
  };

  const save = async () => {
    if (saveState === 'saving') return;
    setSaveState('saving');
    try {
      const saved = await settingsApi.save(settings);
      setSettings(saved);
      // Gaia's voice has no Windows volume slider of its own (WebView2 is
      // the audio host), so the saved gain is applied to playback here.
      setSpeechGain(saved.audio || {});
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

  /** One MCP server's form. Desktop Commander is not removable: it must always exist. */
  function renderMcpServer(index, removable) {
    const server = mcpServers()[index];
    if (!server) return null;
    return (
      <div className="mcp-server">
          <label className="field field-toggle">
            <input
              type="checkbox"
              checked={server.enabled ?? true}
              onChange={(e) => patchMcpServer(index, { enabled: e.target.checked })}
            />
            <span>{L.settingsMcpEnabled}</span>
          </label>
          <label className="field">
            <span>{L.settingsMcpName}</span>
            <input
              type="text"
              value={server.name || ''}
              placeholder={L.settingsMcpNamePlaceholder}
              onChange={(e) => patchMcpServer(index, { name: e.target.value })}
            />
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
            <button
              onClick={() => testServer(index)}
              disabled={!(server.command || '').trim() || testState.loading === server.id}
            >
              {testState.loading === server.id ? L.settingsMcpTesting : L.settingsMcpTest}
            </button>
            {testState.serverId === server.id && testState.result && (
              <span className="capability-line">
                {`${L.settingsMcpTestOk}${testState.result.name ? `: ${testState.result.name}` : ''} (${testState.result.toolCount} ${L.settingsMcpTestTools})`}
              </span>
            )}
            {testState.serverId === server.id && testState.error && (
              <span className="capability-line">{L.settingsMcpTestFailed}</span>
            )}
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
            {removable && (
              <button onClick={() => removeMcpServer(index)}>{L.settingsMcpRemove}</button>
            )}
          </div>
      </div>
    );
  }

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.settingsTitle}</h2>

        <div className="settings-tabs" role="tablist">
          {['general', 'tools'].map((id) => (
            <button
              key={id}
              role="tab"
              aria-selected={tab === id}
              className={`settings-tab${tab === id ? ' active' : ''}`}
              onClick={() => setTab(id)}
            >
              {id === 'general' ? L.settingsTabGeneral : L.settingsTabTools}
            </button>
          ))}
        </div>

        {tab === 'general' && (
        <div className="settings-columns">
          <div className="settings-col">
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
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={settings.audio?.speakReplies ?? true}
                  onChange={(e) => patch({ audio: { ...settings.audio, speakReplies: e.target.checked } })}
                />
                <span>{L.settingsSpeakReplies}</span>
              </label>
              <p className="capability-line">{L.settingsSpeakRepliesHint}</p>
              <label className="field">
                <span>{L.settingsOutputDevice}</span>
                <select
                  value={settings.audio?.outputDevice || ''}
                  onFocus={refreshOutputs}
                  onChange={(e) => {
                    const id = e.target.value || null;
                    patch({ audio: { ...settings.audio, outputDevice: id } });
                    speechApi.selectOutputDevice(id).catch(() => {});
                  }}
                >
                  <option value="">{L.settingsMicDefault}</option>
                  {outputDevices.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field">
                <span>{L.settingsVolume}</span>
                <div className="volume-row">
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.05"
                    value={settings.audio?.volume ?? 1}
                    onChange={(e) => {
                      const volume = Number(e.target.value);
                      patch({ audio: { ...settings.audio, volume } });
                      setSpeechGain({ volume }); // audible at once, even mid-reply; Save keeps it
                    }}
                  />
                  <span className="volume-value">
                    {Math.round((settings.audio?.volume ?? 1) * 100)}%
                  </span>
                </div>
              </label>
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={settings.audio?.muted ?? false}
                  onChange={(e) => {
                    patch({ audio: { ...settings.audio, muted: e.target.checked } });
                    setSpeechGain({ muted: e.target.checked });
                  }}
                />
                <span>{L.settingsMuted}</span>
              </label>
            </section>
          </div>

          <div className="settings-col">
            <section>
              <h3>{L.settingsMicrophone}</h3>
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={voice.status ? voice.status.micEnabled : false}
                  disabled={!voice.status}
                  onChange={(e) => voice.setEnabled(e.target.checked)}
                />
                <span>{L.settingsMicToggle}</span>
              </label>
              <label className="field">
                <span>{L.settingsMicDevice}</span>
                <select
                  value={settings.voice?.inputDevice || ''}
                  onFocus={voice.refreshDevices}
                  onChange={(e) => {
                    const id = e.target.value || null;
                    patch({ voice: { ...settings.voice, inputDevice: id } });
                    voice.selectDevice(id);
                  }}
                >
                  <option value="">{L.settingsMicDefault}</option>
                  {voice.devices.map((d) => (
                    <option key={d.id} value={d.id}>
                      {d.name}
                    </option>
                  ))}
                </select>
              </label>
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={settings.voice?.continuousEnabled ?? false}
                  onChange={(e) => setContinuous(e.target.checked)}
                />
                <span>{L.settingsContinuous}</span>
              </label>
              <p className="capability-line">{L.settingsContinuousHint}</p>
              {listenState !== 'off' && listenState !== 'waiting' && (
                <p className="capability-line mic-note">
                  {listenState === 'hearing' ? L.settingsContinuousHearing : L.settingsContinuousOn}
                </p>
              )}
              {renderWake()}
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={settings.voice?.bargeInEnabled ?? true}
                  onChange={(e) => setBargeIn(e.target.checked)}
                />
                <span>{L.settingsBargeIn}</span>
              </label>
              <p className="capability-line">{L.settingsBargeInHint}</p>
              {(settings.voice?.bargeInEnabled ?? true) && (
                <>
                  <label className="field">
                    <span>{L.settingsBargeInLevel}</span>
                    <div className="volume-row">
                      <span className="capability-line">{L.settingsBargeInLevelEasy}</span>
                      <input
                        type="range"
                        aria-label={L.settingsBargeInLevel}
                        min="0.01"
                        max="0.3"
                        step="0.005"
                        value={bargeInLevel}
                        onChange={(e) => patch({ voice: { ...settings.voice, bargeInLevel: Number(e.target.value) } })}
                        onMouseUp={commitBargeInLevel}
                        onKeyUp={commitBargeInLevel}
                        onBlur={commitBargeInLevel}
                      />
                      <span className="capability-line">{L.settingsBargeInLevelFirm}</span>
                    </div>
                  </label>
                  <p className="capability-line">{L.settingsBargeInLevelHint}</p>
                </>
              )}
              {voice.status && voice.status.state === 'listening' && (
                <>
                  <p className="capability-line">
                    {L.settingsMicListening} {voice.status.device ? voice.status.device.name : ''}
                  </p>
                  <div className="mic-level" aria-hidden="true">
                    <div className="mic-level-fill" style={{ width: `${Math.min(100, Math.round(voice.level * 140))}%` }} />
                  </div>
                </>
              )}
              {voice.status && voice.status.state === 'off' && (
                <p className="capability-line">{L.settingsMicOffState}</p>
              )}
              {micMessage(voice) && <p className="capability-line mic-note">{micMessage(voice)}</p>}
            </section>

            <section>
              <h3>{L.settingsCapabilities}</h3>
              <p className="capability-line">
                {L.settingsMicrophone}: {voice.status ? voice.status.permission : audio ? audio.permission : '—'}
              </p>
              <p className="capability-line">
                {L.settingsCaptureSources}:{' '}
                {captureSources.length === 0
                  ? L.settingsCaptureNone
                  : captureSources.map((s) => s.name).join(', ')}
              </p>
            </section>
          </div>
        </div>
        )}

        {tab === 'tools' && (
          <>
            <div className="mcp-tabs" role="tablist">
              <button
                role="tab"
                aria-selected={isDcSelected}
                className={`mcp-tab${isDcSelected ? ' active' : ''}${mcpServers()[dcIndex]?.enabled === false ? ' off' : ''}`}
                onClick={() => setMcpSel(dcId)}
              >
                {L.settingsDesktopCommander}
              </button>
              {mcpServers().map((server, index) =>
                index === dcIndex ? null : (
                  <button
                    key={server.id || index}
                    role="tab"
                    aria-selected={index === mcpSelectedIndex}
                    className={`mcp-tab${index === mcpSelectedIndex ? ' active' : ''}${server.enabled === false ? ' off' : ''}`}
                    onClick={() => setMcpSel(server.id)}
                  >
                    {server.name || server.command || L.settingsMcpNewServer}
                  </button>
                ),
              )}
              <button className="mcp-tab mcp-tab-add" onClick={addMcpServer} title={L.settingsMcpAdd} aria-label={L.settingsMcpAdd}>
                +
              </button>
            </div>

            {isDcSelected ? (
              <div className="settings-columns">
                <div className="settings-col">
                  <section>
              <h3>{L.settingsDesktop}</h3>
              <p className="capability-line">{L.settingsDesktopHint}</p>
              <label className="field field-toggle">
                <input
                  type="checkbox"
                  checked={desktopSettings().enabled ?? false}
                  onChange={(e) => patchDesktop({ enabled: e.target.checked })}
                />
                <span>{L.settingsDesktopEnabled}</span>
              </label>
              <label className="field">
                <span>{L.settingsDesktopDirs}</span>
                <textarea
                  rows={3}
                  value={(desktopSettings().allowedDirs || []).join('\n')}
                  placeholder="C:\Users\jij\Documents"
                  onChange={(e) => patchDesktop({ allowedDirs: lines(e.target.value) })}
                />
              </label>
              <label className="field">
                <span>{L.settingsDesktopBlocked}</span>
                <textarea
                  rows={4}
                  value={(desktopSettings().blockedCommands || []).join('\n')}
                  onChange={(e) => patchDesktop({ blockedCommands: lines(e.target.value) })}
                />
              </label>
            </section>
                </div>

                <div className="settings-col">
                  <section>
                    <h3>{L.settingsDesktopCommander}</h3>
                    <p className="capability-line">{L.settingsDesktopCommanderHint}</p>
                    {renderMcpServer(dcIndex, false)}
                  </section>
                </div>
              </div>
            ) : (
              <div className="settings-columns">
                <div className="settings-col settings-col-narrow">
                  <section>
                    <h3>{L.settingsMcp}</h3>
                    <p className="capability-line">{L.settingsMcpHint}</p>
                    {renderMcpServer(mcpSelectedIndex, true)}
                  </section>
                </div>
              </div>
            )}
          </>
        )}

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
