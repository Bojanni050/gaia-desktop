import React from 'react';
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent, waitFor, cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import WakeEnrollment from './WakeEnrollment';
import { L } from '../lib/lexicon';

afterEach(cleanup);

function fakeApi({ micEnabled = true, reports = [], finish } = {}) {
  const queue = [...reports];
  return {
    getStatus: vi.fn(async () => ({ micEnabled, state: micEnabled ? 'listening' : 'off' })),
    listDevices: vi.fn(async () => []),
    setMicEnabled: vi.fn(async () => ({ micEnabled: true, state: 'listening' })),
    onState: vi.fn(async () => () => {}),
    onLevel: vi.fn(async () => () => {}),
    onError: vi.fn(async () => () => {}),
    wakeEnrollRecord: vi.fn(async () => queue.shift()),
    wakeEnrollFinish: vi.fn(finish || (async () => ({ enrolled: true, sensitivity: 0.5 }))),
    wakeEnrollCancel: vi.fn(async () => {}),
  };
}

const good = (accepted) => ({ check: { ok: true }, accepted, min: 5, max: 8 });

describe('WakeEnrollment', () => {
  it('tells the user to switch the microphone on first', async () => {
    const api = fakeApi({ micEnabled: false });
    render(<WakeEnrollment api={api} onClose={() => {}} />);
    expect(await screen.findByText(L.wakeNeedMic)).toBeTruthy();
    expect(api.wakeEnrollRecord).not.toHaveBeenCalled();
  });

  it('explains why a recording was rejected and does not count it', async () => {
    const api = fakeApi({ reports: [{ check: { ok: false, problem: 'tooQuiet' }, accepted: 0, min: 5, max: 8 }] });
    render(<WakeEnrollment api={api} onClose={() => {}} />);
    fireEvent.click(await screen.findByText(L.wakeRecord));
    expect(await screen.findByText(L.wakeProblemQuiet)).toBeTruthy();
    expect(screen.getByText(L.wakePromptNormal)).toBeTruthy();
  });

  it('only allows finishing after the minimum number of good clips, then reports the result', async () => {
    const api = fakeApi({ reports: [1, 2, 3, 4, 5].map(good) });
    const onDone = vi.fn();
    const onClose = vi.fn();
    render(<WakeEnrollment api={api} onDone={onDone} onClose={onClose} />);
    const finish = await screen.findByText(L.wakeFinish);
    expect(finish.disabled).toBe(true);
    for (let i = 0; i < 5; i++) {
      fireEvent.click(await screen.findByText(i === 0 ? L.wakeRecord : L.wakeRecordNext));
      await waitFor(() => expect(api.wakeEnrollRecord).toHaveBeenCalledTimes(i + 1));
      await screen.findByText(L.wakeClipGood);
    }
    await waitFor(() => expect(screen.getByText(L.wakeFinish).disabled).toBe(false));
    fireEvent.click(screen.getByText(L.wakeFinish));
    await waitFor(() => expect(onDone).toHaveBeenCalledWith({ enrolled: true, sensitivity: 0.5 }));
    expect(onClose).toHaveBeenCalled();
  });

  it('throws the recordings away when it is closed', async () => {
    const api = fakeApi();
    const { unmount } = render(<WakeEnrollment api={api} onClose={() => {}} />);
    await screen.findByText(L.wakeEnrolTitle);
    unmount();
    expect(api.wakeEnrollCancel).toHaveBeenCalled();
  });
});
