import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react';

// The rail reaches the Rust bridge through server/api.js; nothing here may
// touch a real Tauri command. `invoke` answers the cognition list request.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import UnderstandingRail from './UnderstandingRail';

const list = (ids) => ({
  body: {
    hypotheses: ids.map((id) => ({ id, statement: `s-${id}`, status: 'proposed', scope: 'micro' })),
  },
});

const railOf = (container) => container.querySelector('.understanding-rail');

beforeEach(() => {
  localStorage.clear();
  invoke.mockReset();
});

afterEach(cleanup);

describe('UnderstandingRail', () => {
  it('pulses while something unseen is waiting', async () => {
    invoke.mockResolvedValue(list(['h1']));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    await waitFor(() => expect(railOf(container).className).toContain('has-new'));
  });

  it('rests when the list is empty', async () => {
    invoke.mockResolvedValue(list([]));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(railOf(container).className).not.toContain('has-new');
  });

  it('does not pulse for statements already seen', async () => {
    localStorage.setItem('gaia.understanding.seen', JSON.stringify(['h1']));
    invoke.mockResolvedValue(list(['h1']));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    await waitFor(() => expect(invoke).toHaveBeenCalled());
    expect(railOf(container).className).not.toContain('has-new');
  });

  it('opening the drawer marks everything seen and stops the pulse', async () => {
    invoke.mockResolvedValue(list(['h1', 'h2']));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    await waitFor(() => expect(railOf(container).className).toContain('has-new'));

    fireEvent.click(railOf(container));

    await waitFor(() => expect(container.querySelector('.understanding-drawer')).toBeTruthy());
    await waitFor(() => expect(railOf(container).className).not.toContain('has-new'));
    expect(JSON.parse(localStorage.getItem('gaia.understanding.seen'))).toEqual(['h1', 'h2']);
  });

  it('renders the review cards inside the drawer', async () => {
    invoke.mockResolvedValue(list(['h1']));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    fireEvent.click(railOf(container));
    await waitFor(() => expect(screen.getByText('s-h1')).toBeTruthy());
  });
});
