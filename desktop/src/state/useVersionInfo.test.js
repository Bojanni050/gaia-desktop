/**
 * Tests for useVersionInfo hook.
 * 
 * Tests:
 * 4. Desktop has its own build metadata
 * 5. Desktop correctly displays both builds
 * 6. Cloud unavailable does not prevent Desktop startup
 * 7. Version information is not fetched repeatedly unnecessarily
 * 9. A Cloud/Desktop build mismatch is clearly visible
 * 10. Existing functionality remains unchanged
 */

import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useVersionInfo } from './useVersionInfo';

// Mock serverApi
const mockServerApi = {
  getDesktopVersion: vi.fn(),
  getCloudVersion: vi.fn(),
};

// Mock the serverApi module
vi.mock('../server/api', () => ({
  serverApi: mockServerApi,
}));

describe('useVersionInfo', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('4. Desktop has its own build metadata', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608282100',
      commit: null,
    };
    
    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockResolvedValue(null);

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    expect(result.current.desktopVersion).toEqual(desktopMeta);
    expect(result.current.desktopBuild).toBe('202608282100');
  });

  it('5. Desktop correctly displays both builds', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608271233',
      commit: null,
    };
    
    const cloudMeta = {
      name: 'Gaia Cloud',
      version: '0.8.0',
      build: '202608282334',
      commit: 'abc1234',
    };

    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockResolvedValue(cloudMeta);

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    expect(result.current.desktopVersionString).toBe('0.1.0 · build 202608271233');
    expect(result.current.cloudVersionString).toBe('0.8.0 · build 202608282334');
  });

  it('6. Cloud unavailable does not prevent Desktop startup', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608271233',
      commit: null,
    };

    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockRejectedValue(new Error('Connection failed'));

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    // Desktop version should still be available
    expect(result.current.desktopVersion).toEqual(desktopMeta);
    // Cloud should show as unavailable
    expect(result.current.cloudStatus).toBe('unavailable');
    expect(result.current.cloudVersion).toBeNull();
  });

  it('7. Version information is not fetched repeatedly unnecessarily', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608271233',
      commit: null,
    };
    
    const cloudMeta = {
      name: 'Gaia Cloud',
      version: '0.8.0',
      build: '202608282334',
      commit: null,
    };

    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockResolvedValue(cloudMeta);

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    // Each API should only be called once during initial load
    expect(mockServerApi.getDesktopVersion).toHaveBeenCalledTimes(1);
    expect(mockServerApi.getCloudVersion).toHaveBeenCalledTimes(1);
  });

  it('9. A Cloud/Desktop build mismatch is clearly visible', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608271233',
      commit: null,
    };
    
    const cloudMeta = {
      name: 'Gaia Cloud',
      version: '0.8.0',
      build: '202608282334',
      commit: null,
    };

    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockResolvedValue(cloudMeta);

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    // Builds should NOT match
    expect(result.current.buildsMatch).toBe(false);
    expect(result.current.desktopBuild).toBe('202608271233');
    expect(result.current.cloudBuild).toBe('202608282334');
  });

  it('10. Matching builds are correctly identified', async () => {
    const desktopMeta = {
      name: 'Gaia Desktop',
      version: '0.1.0',
      build: '202608282334',
      commit: null,
    };
    
    const cloudMeta = {
      name: 'Gaia Cloud',
      version: '0.8.0',
      build: '202608282334',
      commit: null,
    };

    mockServerApi.getDesktopVersion.mockResolvedValue(desktopMeta);
    mockServerApi.getCloudVersion.mockResolvedValue(cloudMeta);

    const { result, waitForNextUpdate } = renderHook(() => useVersionInfo());
    
    await waitForNextUpdate();

    // Builds should match
    expect(result.current.buildsMatch).toBe(true);
  });
});
