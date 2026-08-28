/**
 * About Panel - displays version and build information for both Desktop and Cloud.
 * 
 * This panel shows:
 * - Desktop version and build ID
 * - Cloud version and build ID (if connected)
 * - Clear indication of whether builds match
 */
import React from 'react';
import { useVersionInfo } from '../state/useVersionInfo';
import { L } from '../lib/lexicon';

export default function AboutPanel({ onClose }) {
  const versionInfo = useVersionInfo();

  return (
    <div className="settings-overlay" onClick={onClose}>
      <div className="settings-panel about-panel" onClick={(e) => e.stopPropagation()}>
        <h2>{L.aboutTitle}</h2>

        <div className="version-info">
          <section className="version-section">
            <h3>{L.aboutCloud}</h3>
            <div className="version-details">
              <p className="version-line">
                {versionInfo.cloudStatus === 'connected' ? (
                  <>
                    <span className="version-value">{versionInfo.cloudVersionString}</span>
                    <span className="status-badge connected">{L.aboutConnected}</span>
                  </>
                ) : (
                  <span className="version-value unavailable">{L.aboutUnavailable}</span>
                )}
              </p>
            </div>
          </section>

          <section className="version-section">
            <h3>{L.aboutDesktop}</h3>
            <div className="version-details">
              <p className="version-line">
                <span className="version-value">{versionInfo.desktopVersionString}</span>
              </p>
            </div>
          </section>

          {/* Build mismatch indicator */}
          {versionInfo.cloudStatus === 'connected' && versionInfo.cloudBuild && (
            <div className="build-comparison">
              {versionInfo.buildsMatch ? (
                <p className="match-indicator match">
                  {L.aboutBuild} {L.aboutConnected}
                </p>
              ) : (
                <p className="match-indicator mismatch">
                  {L.aboutCloud} {L.aboutBuild}: {versionInfo.cloudBuild}
                  <br />
                  {L.aboutDesktop} {L.aboutBuild}: {versionInfo.desktopBuild}
                </p>
              )}
            </div>
          )}
        </div>

        <div className="settings-actions">
          <button onClick={onClose}>{L.aboutClose}</button>
        </div>
      </div>
    </div>
  );
}
