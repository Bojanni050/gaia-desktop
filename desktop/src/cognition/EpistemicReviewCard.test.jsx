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
  const onAnswer = vi.fn();
  render(
    <EpistemicReviewCard
      item={props.item}
      busy={props.busy || false}
      onConfirm={onConfirm}
      onReject={onReject}
      onReopen={onReopen}
      onAnswer={onAnswer}
    />,
  );
  return { onConfirm, onReject, onReopen, onAnswer };
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
});

describe('EpistemicReviewCard — a question is answered, not judged', () => {
  const question = (overrides = {}) => item({ kind: 'open_question', status: 'proposed', ...overrides });

  it('gives a yes/no question the Ja/Nee pair as the answer, and keeps Loslaten beside it', () => {
    const q = question({ question_shape: 'yes_no' });
    const { onAnswer, onConfirm } = renderCard({ item: q });

    expect(screen.getByText(L.cognitionAskAnswer)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    expect(onAnswer).toHaveBeenCalledWith(q, 'yes');
    fireEvent.click(screen.getByLabelText(L.cognitionNo));
    expect(onAnswer).toHaveBeenCalledWith(q, 'no');
    expect(onConfirm).not.toHaveBeenCalled(); // a question is never confirmed
  });

  it('offers the options the server sent as the choices', () => {
    const q = question({ question_shape: 'choice', question_options: ['openen', 'inhouden'] });
    const { onAnswer, onReject } = renderCard({ item: q });

    expect(screen.queryByLabelText(L.cognitionYes)).toBeNull();
    fireEvent.click(screen.getByText('inhouden'));
    expect(onAnswer).toHaveBeenCalledWith(q, 'inhouden');

    fireEvent.click(screen.getByLabelText(L.cognitionReject));
    expect(onReject).toHaveBeenCalledWith(q);
  });

  it("takes an open question in the person's own words", () => {
    const q = question({ question_shape: 'open' });
    const { onAnswer } = renderCard({ item: q });

    const submit = screen.getByLabelText(L.cognitionAnswerAction);
    expect(submit.disabled).toBe(true); // nothing typed yet

    fireEvent.change(screen.getByLabelText(L.cognitionAnswerPlaceholder), {
      target: { value: '  openen  ' },
    });
    expect(submit.disabled).toBe(false);
    fireEvent.click(submit);
    expect(onAnswer).toHaveBeenCalledWith(q, 'openen');
  });

  it('falls back to the text field for an unknown shape and for a choice without options', () => {
    renderCard({ item: question({ question_shape: 'whatever' }) });
    expect(screen.getByLabelText(L.cognitionAnswerPlaceholder)).toBeTruthy();

    cleanup();

    renderCard({ item: question({ question_shape: 'choice', question_options: ['openen'] }) });
    expect(screen.getByLabelText(L.cognitionAnswerPlaceholder)).toBeTruthy();
  });

  it('carries no macro friction — a question has nothing to establish', () => {
    const q = question({ question_shape: 'yes_no', scope: 'macro', counter_hypothesis: 'an objection' });
    const { onAnswer } = renderCard({ item: q });

    fireEvent.click(screen.getByLabelText(L.cognitionYes));
    expect(onAnswer).toHaveBeenCalledWith(q, 'yes');
    expect(screen.queryByText(L.cognitionConfirm)).toBeNull();
  });

  it('locks the answers while busy', () => {
    renderCard({ item: question({ question_shape: 'yes_no' }), busy: true });
    expect(screen.getByLabelText(L.cognitionYes).disabled).toBe(true);
    expect(screen.getByLabelText(L.cognitionReject).disabled).toBe(true);
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

  it('a confirmed statement is settled: the verdict stays, the answers are gone', () => {
    const h = item({ status: 'confirmed', counter_hypothesis: 'the opposite reading' });
    const { onConfirm, onReject } = renderCard({ item: h });

    expect(screen.getByText(L.cognitionConfirmed)).toBeTruthy();
    expect(screen.queryByLabelText(L.cognitionYes)).toBeNull();
    expect(screen.queryByLabelText(L.cognitionNo)).toBeNull();
    expect(screen.queryByText(L.cognitionAsk)).toBeNull();

    // what was weighed can still be read, but nothing can be clicked again
    fireEvent.click(screen.getByText(L.cognitionMore));
    expect(screen.getByText('the opposite reading')).toBeTruthy();
    expect(onConfirm).not.toHaveBeenCalled();
    expect(onReject).not.toHaveBeenCalled();
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
