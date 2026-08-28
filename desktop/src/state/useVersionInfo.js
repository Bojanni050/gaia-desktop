/**
 * Version information hook for Gaia Desktop.
 * 
 * Manages both Desktop and Cloud build metadata, fetching Cloud version
 * from the server during initialization and caching it for the session.
 */
import { useEffect, useState, useCallback } from 'react';
import { serverApi } from '../server/api';

export function useVersionInfo() {
  const [desktopVersion, setDesktopVersion] = useState(null);
  const [cloudVersion, setCloudVersion] = useState(null);
  const [cloudStatus, setCloudStatus] = useState('loading'); // loading | connected | unavailable
  const [loading, setLoading] = useState(true);

  // Fetch Desktop build metadata from Rust
  const fetchDesktopVersion = useCallback(async () => {
    try {
      const desktopMeta = await serverApi.getDesktopVersion();
      setDesktopVersion(desktopMeta);
    } catch (error) {
      console.warn('[version] Failed to get Desktop version:', error);
      // Fallback for development
      setDesktopVersion({
        name: 'Gaia Desktop',
        version: '0.1.0',
        build: new Date().toISOString().slice(0, 16).replace('T', '').replace(/-/g, '').replace(':', '') + '-dev',
        commit: null
      });
    }
  }, []);

  // Fetch Cloud version from server
  const fetchCloudVersion = useCallback(async () => {
    try {
      const cloudMeta = await serverApi.getCloudVersion();
      setCloudVersion(cloudMeta);
      setCloudStatus('connected');
    } catch (error) {
      console.warn('[version] Failed to get Cloud version:', error.message);
      setCloudStatus('unavailable');
      setCloudVersion(null);
    }
  }, []);

  // Initial load
  useEffect(() => {
    let active = true;
    
    const load = async () => {
      await fetchDesktopVersion();
      await fetchCloudVersion();
      if (active) setLoading(false);
    };
    
    load();

    return () => {
      active = false;
    };
  }, [fetchDesktopVersion, fetchCloudVersion]);

  // Refresh both versions
  const refresh = useCallback(async () => {
    setLoading(true);
    await fetchDesktopVersion();
    await fetchCloudVersion();
    setLoading(false);
  }, [fetchDesktopVersion, fetchCloudVersion]);

  return {
    desktopVersion,
    cloudVersion,
    cloudStatus,
    loading,
    refresh,
    // Convenience getters
    get desktopBuild() {
      return desktopVersion?.build || 'unknown';
    },
    get desktopVersionString() {
      return desktopVersion ? `${desktopVersion.version} \u00b7 build ${desktopVersion.build}` : 'unknown';
    },
    get cloudBuild() {
      return cloudVersion?.build || null;
    },
    get cloudVersionString() {
      return cloudVersion ? `${cloudVersion.version} \u00b7 build ${cloudVersion.build}` : null;
    },
    // Check if builds match
    get buildsMatch() {
      return desktopVersion?.build && cloudVersion?.build && 
             desktopVersion.build === cloudVersion.build;
    },
    // Format for display
    formatVersionLabel: (prefix) => {
      if (prefix === 'cloud') {
        if (cloudStatus === 'unavailable') return 'unavailable';
        if (cloudStatus === 'loading') return 'loading...';
        return cloudVersionString || 'unknown';
      }
      return desktopVersionString || 'unknown';
    }
  };
}
