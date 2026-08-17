/**
 * Settings — this device's behaviour only: which Gaia Cloud server to reach,
 * notifications, quiet presence, and the local capability surfaces (audio,
 * capture). Nothing cognitive ever appears here.
 */
import React, { useEffect, useState } from 'react';
import { settingsApi, serverApi, audioApi, captureApi } from '../server/api';

export default function SettingsPanel({ onClose, quiet, onQuietChange }) {
  const [settings, setSettings] = useState(null);
  const [audio, setAudio] = useState(null);
  const [captureSources, setCaptureSources] = useState([]);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState(null);
  const [saving, setSaving] = useState(false);
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
        <div className="settings-panel">Loading…</div>
      </div>
    );
  }

  const patch = (part) => setSettings((prev) => ({ ...prev, ...part }));

  const save = async () => {
    setSaving(true);
    try {
      const saved = await settingsApi.save(settings);
      setSettings(saved);
    } finally {
      setSaving(false);
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
        <h2>Settings</h2>

        <section>
          <h3>Gaia Cloud</h3>
          <label className="field">
            <span>Server URL</span>
            <input
              type="url"
              value={settings.server?.baseUrl || ''}
              placeholder="https://gaia.example/api"
              onChange={(e) => patch({ server: { ...settings.server, baseUrl: e.target.value || null } })}
            />
          </label>
          <label className="field">
            <span>Auth token</span>
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
                title={showToken ? 'Hide token' : 'Show token'}
              >
                {showToken ? 'Hide' : 'Show'}
              </button>
            </div>
          </label>
          <div className="field-row">
            <button onClick={testConnection} disabled={testing}>
              {testing ? 'Testing…' : 'Test connection'}
            </button>
            {testResult && <span className={`test-result test-${testResult}`}>{String(testResult)}</span>}
          </div>
        </section>

        <section>
          <h3>Behaviour</h3>
          <label className="field field-toggle">
            <input
              type="checkbox"
              checked={settings.notifications?.enabled ?? true}
              onChange={(e) => patch({ notifications: { enabled: e.target.checked } })}
            />
            <span>Notifications</span>
          </label>
          <label className="field field-toggle">
            <input type="checkbox" checked={quiet} onChange={(e) => onQuietChange(e.target.checked)} />
            <span>Quiet presence (do not disturb)</span>
          </label>
        </section>

        <section>
          <h3>Local capabilities</h3>
          <p className="capability-line">Microphone: {audio ? audio.permission : 'unknown'}</p>
          <p className="capability-line">
            Capture sources: {captureSources.length === 0 ? 'none registered yet' : captureSources.map((s) => s.name).join(', ')}
          </p>
        </section>

        <div className="settings-actions">
          <button className="primary" onClick={save} disabled={saving}>
            {saving ? 'Saving…' : 'Save'}
          </button>
          <button onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}
