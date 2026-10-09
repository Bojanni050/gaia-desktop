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
  const onReopen = vi.fn();
  render(
    <EpistemicReviewCard
      item={props.item}
      busy={props.busy || false}
      onConfirm={onConfirm}
      onReject={onReject}
      onReopen={onReopen}
    />,
  );
  return { onConfirm, onReject, onReopen };
}

describe('EpistemicReviewCard — one question, two answers', () => {
  it('asks a plain question and confirms a micro statement in one act', () => {
    const h = item({ scope: 'micro' });
    const { onConfirm } = renderCard({ item: h });

    expect(screen.getByText(L.cognitionAsk)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    expect(onConfirm).toHaveBeenCalledWith(h, { rationale: undefined, statement: undefined });
  });

  it('lets a statement go with Nee', () => {
    const h = item();
    const { onReject } = renderCard({ item: h });
    fireEvent.click(screen.getByLabelText(L.cognitionNo));
    expect(onReject).toHaveBeenCalledWith(h);
  });

  it('hides the objection and the evidence note behind Meer', () => {
    const h = item({
      status: 'proposed',
      counter_hypothesis: 'the opposite reading',
      evidence_for: ['e1'],
      evidence_against: ['e2'],
    });
    renderCard({ item: h });

    // Nothing overwhelming up front.
    expect(screen.queryByText('the opposite reading')).toBeNull();
    expect(screen.queryByText(L.cognitionEvidenceForNote)).toBeNull();

    fireEvent.click(screen.getByText(L.cognitionMore));
    expect(screen.getByText('the opposite reading')).toBeTruthy();
    expect(screen.getByText(L.cognitionCounterHint)).toBeTruthy();
    expect(screen.getByText(L.cognitionEvidenceForNote)).toBeTruthy();
    expect(screen.getByText(L.cognitionEvidenceAgainstNote)).toBeTruthy();
  });

  it('offers no Meer when there is nothing to reveal', () => {
    renderCard({ item: item({ counter_hypothesis: null }) });
    expect(screen.queryByText(L.cognitionMore)).toBeNull();
    expect(screen.queryByText(L.cognitionAsk)).toBeTruthy(); // still just the question
  });
});

describe('EpistemicReviewCard — the question follows the kind', () => {
  it('asks the plain question for a hypothesis', () => {
    renderCard({ item: item({ kind: 'hypothesis' }) });
    expect(screen.getByText(L.cognitionAsk)).toBeTruthy();
  });

  it('asks about the link for a relationship', () => {
    renderCard({ item: item({ kind: 'relationship' }) });
    expect(screen.getByText(L.cognitionAskRelationship)).toBeTruthy();
    expect(screen.queryByText(L.cognitionAsk)).toBeNull();
  });

  it('asks whether it is still open for an open question', () => {
    renderCard({ item: item({ kind: 'open_question' }) });
    expect(screen.getByText(L.cognitionAskOpenQuestion)).toBeTruthy();
  });
});

describe('EpistemicReviewCard — macro friction as a second step', () => {
  it('does not confirm a macro statement on Ja — it opens the objection first', () => {
    const h = item({ scope: 'macro', counter_hypothesis: 'the user only said that once, in frustration' });
    const { onConfirm } = renderCard({ item: h });

    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    expect(onConfirm).not.toHaveBeenCalled(); // second step, not a one-click confirm
    expect(screen.getByText('the user only said that once, in frustration')).toBeTruthy();

    fireEvent.click(screen.getByLabelText(L.cognitionConfirm));
    expect(onConfirm).toHaveBeenCalledWith(h, {
      rationale: L.cognitionImplicationEstablish,
      statement: undefined,
    });
  });

  it('leaves a macro statement without a counter-hypothesis unconfirmable, and says so', () => {
    const h = item({ scope: 'macro', counter_hypothesis: null });
    renderCard({ item: h });

    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    expect(screen.getByText(L.cognitionCounterMissing)).toBeTruthy();
    expect(screen.getByLabelText(L.cognitionConfirm).disabled).toBe(true);
  });

  it('can back out of the second step', () => {
    const h = item({ scope: 'macro', counter_hypothesis: 'an objection' });
    renderCard({ item: h });
    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    fireEvent.click(screen.getByText(L.cognitionBack));
    expect(screen.getByLabelText(L.cognitionYes)).toBeTruthy(); // back to the question
  });
});

describe('EpistemicReviewCard — the rest', () => {
  it('locks Ja and Nee while busy', () => {
    renderCard({ item: item(), busy: true });
    expect(screen.getByLabelText(L.cognitionYes).disabled).toBe(true);
    expect(screen.getByLabelText(L.cognitionNo).disabled).toBe(true);
  });

  it('a rejected statement offers only a reasoned Reconsider', () => {
    const h = item({ status: 'rejected' });
    const { onReopen, onConfirm } = renderCard({ item: h });

    expect(screen.queryByLabelText(L.cognitionYes)).toBeNull();
    expect(screen.queryByLabelText(L.cognitionNo)).toBeNull();

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
});
