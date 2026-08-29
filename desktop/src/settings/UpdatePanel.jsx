/**
 * Update Panel - displays update information and allows checking for updates.
 * Uses the Tauri updater plugin to check for and install updates.
 */
import React, { useState, useEffect, useCallback, useRef } from 'react';
import { check } from '@tauri-apps/plugin-updater';
import { Info, Check, X, Loader2 } from 'lucide-react';
import { L } from '../lib/lexicon';

export default function UpdatePanel({ onClose }) {
  const [status, setStatus] = useState('idle'); // idle | checking | available | not-available | error | installing
  const [version, setVersion] = useState(null);
  const [error, setError] = useState(null);
  const [progress, setProgress] = useState(0);
  // Tauri v2's updater returns the Update instance itself from check() —
  // downloadAndInstall() is a method on it, not a separate module-level call.
  const updateRef = useRef(null);

  const checkForUpdates = useCallback(async () => {
    setStatus('checking');
    setError(null);

    try {
      const update = await check();

      if (update) {
        updateRef.current = update;
        setStatus('available');
        setVersion(update.version);
      } else {
        setStatus('not-available');
      }
    } catch (err) {
      setStatus('error');
      setError(err.message || String(err));
    }
  }, []);

  const handleInstall = useCallback(async () => {
    if (!updateRef.current) return;
    setStatus('installing');
    setError(null);
    setProgress(0);

    let totalBytes = 0;
    let downloadedBytes = 0;

    try {
      await updateRef.current.downloadAndInstall((event) => {
        if (event.event === 'Started') {
          totalBytes = event.data.contentLength || 0;
        } else if (event.event === 'Progress') {
          downloadedBytes += event.data.chunkLength;
          setProgress(totalBytes > 0 ? (downloadedBytes / totalBytes) * 100 : 0);
        } else if (event.event === 'Finished') {
          setProgress(100);
        }
      });
      setStatus('restart-required');
    } catch (err) {
      setStatus('error');
      setError(err.message || String(err));
    }
  }, []);

  const handleRestart = useCallback(async () => {
    // Tauri updater will handle the restart automatically
    // after successful installation
    window.location.reload();
  }, []);

  useEffect(() => {
    // Auto-check for updates when panel opens
    checkForUpdates();
  }, [checkForUpdates]);

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel update-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.updateTitle || 'Updates'}</h2>

        {status === 'idle' && (
          <p className="update-hint">{L.updateHint || 'Checking for updates...'}</p>
        )}

        {status === 'checking' && (
          <div className="update-checking">
            <Loader2 size={20} className="update-spinner" />
            <span>{L.updateChecking || 'Checking for updates...'}</span>
          </div>
        )}

        {status === 'available' && (
          <div className="update-available">
            <Check size={24} className="update-icon" />
            <p className="update-version">
              {L.updateAvailable || 'Update available'}: <strong>v{version}</strong>
            </p>
            <p className="update-description">
              {L.updateDescription || 'A new version is ready to install.'}
            </p>
            <button 
              className="primary update-btn" 
              onClick={handleInstall}
              disabled={status === 'installing'}
            >
              {L.updateInstall || 'Install Update'}
            </button>
          </div>
        )}

        {status === 'not-available' && (
          <div className="update-not-available">
            <Info size={24} className="update-icon" />
            <p>{L.updateNotAvailable || 'You have the latest version.'}</p>
            <button className="update-btn" onClick={checkForUpdates}>
              {L.updateCheckAgain || 'Check Again'}
            </button>
          </div>
        )}

        {status === 'installing' && (
          <div className="update-installing">
            <Loader2 size={24} className="update-spinner" />
            <p>{L.updateInstalling || 'Installing update...'}</p>
            <div className="update-progress-bar">
              <div 
                className="update-progress-fill" 
                style={{ width: `${progress}%` }}
              />
            </div>
            <p className="update-progress-text">{Math.round(progress)}%</p>
          </div>
        )}

        {status === 'restart-required' && (
          <div className="update-restart">
            <Check size={24} className="update-icon" />
            <p>{L.updateDownloaded || 'Update downloaded successfully!'}</p>
            <p className="update-restart-hint">
              {L.updateRestartHint || 'The application needs to restart to complete the update.'}
            </p>
            <button className="primary update-btn" onClick={handleRestart}>
              {L.updateRestart || 'Restart Now'}
            </button>
          </div>
        )}

        {status === 'error' && (
          <div className="update-error">
            <X size={24} className="update-icon error" />
            <p className="update-error-title">{L.updateError || 'Update check failed'}</p>
            <p className="update-error-message">{error}</p>
            <button className="update-btn" onClick={checkForUpdates}>
              {L.updateRetry || 'Try Again'}
            </button>
          </div>
        )}

        <div className="settings-actions">
          <button onClick={onClose}>{L.settingsClose || 'Close'}</button>
        </div>
      </div>
    </div>
  );
}
