import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup, fireEvent } from '@testing-library/react';
import ResearchStatus from './ResearchStatus';

afterEach(cleanup);

const JOB = {
  id: 'rj_1',
  conversationId: 'c1',
  status: 'awaiting_approval',
  approval: {
    command: 'python3 scripts/sources.py --write',
    description: 'schrijft de bronnen weg in de cache',
    choices: ['once', 'session', 'always', 'deny'],
  },
};

describe('ResearchStatus', () => {
  it('renders nothing when there is nothing to say', () => {
    const { container } = render(<ResearchStatus activeConversationId="c1" />);
    expect(container.firstChild).toBeNull();
  });

  it('says work is running, quietly', () => {
    render(<ResearchStatus running={[{ id: 'rj_run' }]} activeConversationId="c1" />);
    expect(screen.getByRole('status').textContent).toMatch(/Onderzoek loopt bij Hermes/);
  });

  it('puts the command and Hermes\' own choices in front of the person', () => {
    render(<ResearchStatus awaiting={[JOB]} activeConversationId="c1" onAnswer={vi.fn()} />);

    expect(screen.getByRole('alert').textContent).toMatch(/vraagt om goedkeuring/);
    expect(screen.getByText('python3 scripts/sources.py --write')).toBeTruthy();
    expect(screen.getByText('schrijft de bronnen weg in de cache')).toBeTruthy();
    // Exactly the decisions the server accepts — no invented ones.
    const labels = screen.getAllByRole('button').map((b) => b.textContent);
    expect(labels).toEqual(['Eenmalig', 'Deze sessie', 'Altijd', 'Weigeren']);
  });

  it('hands the chosen decision to the caller, and nothing else', () => {
    const onAnswer = vi.fn(async () => {});
    render(<ResearchStatus awaiting={[JOB]} activeConversationId="c1" onAnswer={onAnswer} />);

    fireEvent.click(screen.getByText('Weigeren'));

    expect(onAnswer).toHaveBeenCalledWith('rj_1', 'deny');
  });

  it('offers only what the job offers, even when that is just once or deny', () => {
    const narrow = { ...JOB, approval: { ...JOB.approval, choices: ['once', 'deny'] } };
    render(<ResearchStatus awaiting={[narrow]} activeConversationId="c1" onAnswer={vi.fn()} />);

    const labels = screen.getAllByRole('button').map((b) => b.textContent);
    expect(labels).toEqual(['Eenmalig', 'Weigeren']);
  });

  it('does not hijack the thread for a result that belongs to another one', () => {
    const onOpen = vi.fn();
    const other = { id: 'rj_2', conversationId: 'c2', status: 'done' };
    render(<ResearchStatus finished={[other]} activeConversationId="c1" onOpen={onOpen} />);

    expect(screen.getByText(/ander gesprek/)).toBeTruthy();
    fireEvent.click(screen.getByText('Openen'));
    expect(onOpen).toHaveBeenCalledWith(other);
  });

  it('says so when the decision could not be passed on', async () => {
    const onAnswer = vi.fn(async () => { throw new Error('server offline'); });
    render(<ResearchStatus awaiting={[JOB]} activeConversationId="c1" onAnswer={onAnswer} />);

    fireEvent.click(screen.getByText('Eenmalig'));

    expect(await screen.findByText('server offline')).toBeTruthy();
  });
});
