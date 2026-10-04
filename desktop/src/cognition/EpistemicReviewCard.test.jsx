import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import EpistemicReviewCard from './EpistemicReviewCard';
import { L } from '../lib/lexicon';

afterEach(cleanup);

function item(overrides = {}) {
  return {
    id: 'h1',
    statement: 'The user prefers bullet points for technical questions.',
    status: 'testing',
    scope: 'micro',
    counter_hypothesis: null,
    evidence_for: [],
    evidence_against: [],
    ...overrides,
  };
}

function renderCard(props) {
  const onConfirm = vi.fn();
  const onReject = vi.fn();
  const onTest = vi.fn();
  const onReopen = vi.fn();
  render(
    <EpistemicReviewCard
      item={props.item}
      busy={props.busy || false}
      onConfirm={onConfirm}
      onReject={onReject}
      onTest={onTest}
      onReopen={onReopen}
    />,
  );
  return { onConfirm, onReject, onTest, onReopen };
}

describe('EpistemicReviewCard friction', () => {
  it('a micro statement confirms in one calm act, with no forced rationale', () => {
    const h = item({ scope: 'micro' });
    const { onConfirm } = renderCard({ item: h });
    const confirm = screen.getByLabelText(L.cognitionConfirm);
    expect(confirm.disabled).toBe(false);
    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith(h, { rationale: undefined, statement: undefined });
  });

  it('a macro statement cannot be confirmed until the counter-hypothesis is opened and the implication answered', () => {
    const h = item({ scope: 'macro', counter_hypothesis: 'the user only said that once, in frustration' });
    const { onConfirm } = renderCard({ item: h });
    const confirm = screen.getByLabelText(L.cognitionConfirm);

    expect(confirm.disabled).toBe(true);
    fireEvent.click(screen.getByText(L.cognitionShowCounter));
    expect(confirm.disabled).toBe(true); // looked, but no implication chosen yet

    // A wrong implication keeps the override locked.
    fireEvent.click(screen.getByLabelText(L.cognitionImplicationCounter));
    expect(confirm.disabled).toBe(true);

    fireEvent.click(screen.getByLabelText(L.cognitionImplicationEstablish));
    expect(confirm.disabled).toBe(false);

    fireEvent.click(confirm);
    expect(onConfirm).toHaveBeenCalledWith(h, {
      rationale: L.cognitionImplicationEstablish,
      statement: undefined,
    });
  });

  it('a macro statement without a counter-hypothesis stays unconfirmable and says so', () => {
    const h = item({ scope: 'macro', counter_hypothesis: null });
    renderCard({ item: h });
    expect(screen.getByText(L.cognitionCounterMissing)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(L.cognitionImplicationEstablish));
    expect(screen.getByLabelText(L.cognitionConfirm).disabled).toBe(true);
  });

  it('Nuanceren sends the human re-wording with the confirmation', () => {
    const h = item({ scope: 'micro' });
    const { onConfirm } = renderCard({ item: h });
    fireEvent.click(screen.getByText(L.cognitionNuance));
    fireEvent.change(screen.getByPlaceholderText(L.cognitionNuancePlaceholder), {
      target: { value: 'prefers bullets only in technical context' },
    });
    fireEvent.click(screen.getByLabelText(L.cognitionConfirm));
    expect(onConfirm).toHaveBeenCalledWith(h, {
      rationale: undefined,
      statement: 'prefers bullets only in technical context',
    });
  });

  it('a busy card locks every action', () => {
    renderCard({ item: item(), busy: true });
    expect(screen.getByLabelText(L.cognitionConfirm).disabled).toBe(true);
    expect(screen.getByLabelText(L.cognitionReject).disabled).toBe(true);
  });

  it('a rejected statement offers only a reasoned Reconsider — never confirm/test/reject', () => {
    const h = item({ status: 'rejected' });
    const { onReopen, onConfirm } = renderCard({ item: h });

    expect(screen.queryByLabelText(L.cognitionConfirm)).toBeNull();
    expect(screen.queryByLabelText(L.cognitionReject)).toBeNull();
    expect(screen.queryByLabelText(L.cognitionTesting)).toBeNull();

    const reopen = screen.getByLabelText(L.cognitionReopen);
    expect(reopen.disabled).toBe(true); // a reason is mandatory
    fireEvent.change(screen.getByPlaceholderText(L.cognitionReopenPlaceholder), {
      target: { value: 'the disproof was retracted' },
    });
    expect(reopen.disabled).toBe(false);

    fireEvent.click(reopen);
    expect(onReopen).toHaveBeenCalledWith(h, 'the disproof was retracted');
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('shows the quarantined counter-hypothesis only on request, with its not-a-fact note', () => {
    const h = item({ scope: 'macro', counter_hypothesis: 'the opposite reading' });
    renderCard({ item: h });
    expect(screen.queryByText('the opposite reading')).toBeNull();
    fireEvent.click(screen.getByText(L.cognitionShowCounter));
    expect(screen.getByText('the opposite reading')).toBeTruthy();
    expect(screen.getByText(L.cognitionCounterHint)).toBeTruthy();
  });
});
