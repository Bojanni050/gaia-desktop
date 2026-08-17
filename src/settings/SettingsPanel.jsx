/**
 * Settings — this device's behaviour only: which Gaia Cloud server to reach,
 * notifications, quiet presence, and the local capability surfaces (audio,
 * capture). Nothing cognitive ever appears here.
 *
 * Saving gives calm feedback: the button keeps its width, settles into a
 * soft "saved" state and quietly returns — never a jump or a flash.
 */
import React, { useEffect, useState } from 'react';
import { settingsApi, serverApi, audioApi, captureApi } from '../server/api';
import { L } from '../lib/lexicon';

export default function SettingsPanel({ onClose, quiet, onQuietChange }) {
  const [settings, setSettings] = useState(null);
  const [audio, setAudio] = useState(null);
  const [captureSources, setCaptureSources] = useState([]);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState(null);
  const [saveState, setSaveState] = useState('idle'); // idle | saving | saved
  const [showToken, setShowToken] = useState(false);

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
