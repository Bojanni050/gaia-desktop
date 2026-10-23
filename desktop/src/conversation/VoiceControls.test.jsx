import React from 'react';
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import VoiceControls from './VoiceControls';
import { L } from '../lib/lexicon';

afterEach(cleanup);

function fakeApi({ micEnabled = false } = {}) {
  let mic = micEnabled;
  return {
    getStatus: vi.fn(async () => ({ micEnabled: mic, state: mic ? 'listening' : 'off' })),
    listDevices: vi.fn(async () => []),
    setMicEnabled: vi.fn(async (enabled) => {
      mic = enabled;
      return { micEnabled: mic, state: mic ? 'listening' : 'off' };
    }),
    continuousSet: vi.fn(async () => true),
    onState: vi.fn(async () => () => {}),
    onLevel: vi.fn(async () => () => {}),
    onGate: vi.fn(async () => () => {}),
    onError: vi.fn(async () => () => {}),
  };
}

describe('VoiceControls', () => {
  it('switches the microphone on and off', async () => {
    const api = fakeApi();
    render(<VoiceControls api={api} listenState="off" />);
    fireEvent.click(await screen.findByRole('button', { name: L.voiceMicSwitchOn }));
    await waitFor(() => expect(api.setMicEnabled).toHaveBeenCalledWith(true));
    fireEvent.click(await screen.findByRole('button', { name: L.voiceMicSwitchOff }));
    await waitFor(() => expect(api.setMicEnabled).toHaveBeenLastCalledWith(false));
  });

  it('switching continuous conversation on also switches the microphone on', async () => {
    const api = fakeApi({ micEnabled: false });
    render(<VoiceControls api={api} listenState="off" />);
    fireEvent.click(await screen.findByRole('button', { name: L.voiceContinuousSwitchOn }));
    await waitFor(() => expect(api.continuousSet).toHaveBeenCalledWith(true));
    await waitFor(() => expect(api.setMicEnabled).toHaveBeenCalledWith(true));
  });

  it('switching continuous conversation off leaves the microphone alone', async () => {
    const api = fakeApi({ micEnabled: true });
    render(<VoiceControls api={api} listenState="listening" />);
    const button = await screen.findByRole('button', { name: L.voiceContinuousSwitchOff });
    expect(button.getAttribute('aria-pressed')).toBe('true');
    fireEvent.click(button);
    await waitFor(() => expect(api.continuousSet).toHaveBeenCalledWith(false));
    expect(api.setMicEnabled).not.toHaveBeenCalled();
  });

  it('waiting for "Hey Gaia" is not continuous conversation', async () => {
    const api = fakeApi({ micEnabled: true });
    render(<VoiceControls api={api} listenState="waiting" />);
    const button = await screen.findByRole('button', { name: L.voiceContinuousSwitchOn });
    expect(button.getAttribute('aria-pressed')).toBe('false');
  });
});
