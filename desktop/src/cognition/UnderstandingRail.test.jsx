import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, cleanup, waitFor, fireEvent } from '@testing-library/react';

// The rail reaches the Rust bridge through server/api.js; nothing here may
// touch a real Tauri command. `invoke` answers the cognition list request.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import UnderstandingRail, { clampWidth, MIN_WIDTH } from './UnderstandingRail';

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

  it('slides the drawer and the rail button as one movement, in and out', async () => {
    invoke.mockResolvedValue(list([]));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    const drawer = () => container.querySelector('.understanding-drawer');
    const moved = () => Boolean(railOf(container).style.right);
    await waitFor(() => expect(invoke).toHaveBeenCalled());

    fireEvent.click(railOf(container));
    // Mounted at its hidden position first; the button has not moved yet either...
    expect(drawer()).toBeTruthy();
    expect(drawer().className).not.toContain(' in');
    expect(moved()).toBe(false);
    // ...and then both start sliding on the same flip.
    await waitFor(() => expect(drawer().className).toContain(' in'));
    expect(moved()).toBe(true);

    fireEvent.click(railOf(container));
    // Closing: both head back at once, and the drawer is kept until its slide has finished.
    expect(drawer()).toBeTruthy();
    expect(drawer().className).not.toContain(' in');
    expect(moved()).toBe(false);
    await waitFor(() => expect(drawer()).toBeNull(), { timeout: 2000 });
  });

  it('renders the review cards inside the drawer', async () => {
    invoke.mockResolvedValue(list(['h1']));
    const { container } = render(<UnderstandingRail pollMs={0} />);
    fireEvent.click(railOf(container));
    await waitFor(() => expect(screen.getByText('s-h1')).toBeTruthy());
  });

  it('answers an open question and drops it from the list', async () => {
    invoke.mockResolvedValueOnce({
      body: {
        hypotheses: [
          {
            id: 'q1',
            statement: 'Zal Bo het bestand verder gebruiken (openen/inhouden)?',
            kind: 'open_question',
            question_shape: 'choice',
            question_options: ['openen', 'inhouden'],
            status: 'proposed',
            scope: 'micro',
          },
        ],
      },
    });
    invoke.mockResolvedValue({ body: { id: 'q1', answer: 'openen' } });
    const { container } = render(<UnderstandingRail pollMs={0} />);
    fireEvent.click(railOf(container));
    await waitFor(() => expect(screen.getByText('inhouden')).toBeTruthy());

    fireEvent.click(screen.getByText('inhouden'));

    // A verdict would stay in the list; an answered question leaves it.
    await waitFor(() => expect(screen.queryByText('inhouden')).toBeNull());
    expect(invoke).toHaveBeenLastCalledWith('server_request', {
      request: {
        method: 'post',
        path: 'cognition/hypotheses/q1/answer',
        body: { answer: 'inhouden' },
      },
    });
  });
});

describe('drawer width', () => {
  it('never grows past a third of the viewport', () => {
    expect(clampWidth(9999, 1000)).toBe(330);
    expect(clampWidth(400, 2000)).toBe(400);
  });

  it('never shrinks below the minimum', () => {
    expect(clampWidth(10, 1000)).toBe(MIN_WIDTH);
  });

  it('fills the third of a narrow window rather than overflow it', () => {
    expect(clampWidth(9999, 500)).toBe(165);
  });
});
