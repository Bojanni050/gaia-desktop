import React from 'react';
import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react';

import MessageView from './MessageView';
import { L } from '../lib/lexicon';

afterEach(cleanup);

const reply = (extra = {}) => ({ id: 'a1', role: 'assistant', content: 'Dat onthoud ik.', createdAt: '2026-10-12T20:35:00.000Z', ...extra });

describe('MessageView: the light for what Gaia kept', () => {
  it('shows a golden light next to her reply when she kept something, saying what', () => {
    render(<MessageView message={reply({ kept: [{ bank: 'bojan', kind: 'preference', text: 'Bo drinkt graag thee' }] })} />);
    const light = screen.getByRole('img', { name: L.memoryKept });
    expect(light.className).toContain('memory-dot');
    expect(light.getAttribute('title')).toContain('Bo drinkt graag thee');
  });

  it('shows nothing when she kept nothing', () => {
    render(<MessageView message={reply()} />);
    expect(screen.queryByRole('img', { name: L.memoryKept })).toBeNull();
  });

  it('never shows it on what the person wrote', () => {
    render(<MessageView message={{ id: 'u1', role: 'user', content: 'onthoud dit', kept: [{ text: 'x' }] }} />);
    expect(screen.queryByRole('img', { name: L.memoryKept })).toBeNull();
  });
});

describe('MessageView: the thought process', () => {
  const withReasoning = reply({ content: 'De vps staat nog op het oude adres.', reasoning: 'Eerst kijken waar hij op luistert.' });

  it('starts closed, so the answer is never pushed around while it is being written', () => {
    render(<MessageView message={withReasoning} />);
    const panel = screen.getByText(L.thoughtProcess).closest('details');
    expect(panel.hasAttribute('open')).toBe(false);
  });

  it('keeps the thought process readable and the answer outside it', () => {
    render(<MessageView message={withReasoning} />);
    const body = screen.getByText(L.thoughtProcess).closest('details').querySelector('.thought-process-body');
    expect(body.textContent).toContain('Eerst kijken waar hij op luistert.');
    // Closing it hides the thinking, never the reply.
    expect(body.textContent).not.toContain('De vps staat nog op het oude adres.');
    expect(screen.getByText('De vps staat nog op het oude adres.')).toBeTruthy();
  });

  it('shows no panel at all when Gaia did not think out loud', () => {
    render(<MessageView message={reply()} />);
    expect(screen.queryByText(L.thoughtProcess)).toBeNull();
  });
});

describe('MessageView: taking a memory back', () => {
  const selectionAt = (text, node) => {
    window.getSelection = () => ({ anchorNode: node, toString: () => text });
  };
  const originalSelection = window.getSelection;

  afterEach(() => {
    window.getSelection = originalSelection;
  });

  it('offers the button on her reply only, and only when the route is wired', () => {
    const onRetract = async () => ({ status: 'retracted', message: 'Taken back.' });
    const { unmount } = render(<MessageView message={reply()} onRetract={onRetract} />);
    expect(screen.getByRole('button', { name: L.retract })).toBeTruthy();
    unmount();

    render(<MessageView message={{ id: 'u1', role: 'user', content: 'onthoud dit' }} onRetract={onRetract} />);
    expect(screen.queryByRole('button', { name: L.retract })).toBeNull();
    cleanup();

    render(<MessageView message={reply()} />);
    expect(screen.queryByRole('button', { name: L.retract })).toBeNull();
  });

  it('takes the marked sentence in her reply as the quote, and sends it with the correction', async () => {
    const calls = [];
    const onRetract = async (...args) => {
      calls.push(args);
      return { status: 'retracted', message: 'Taken back: "Yassine is een vriend van je".' };
    };
    render(<MessageView message={reply()} onRetract={onRetract} />);

    const body = document.querySelector('.msg-body');
    selectionAt('   Yassine is een vriend   van je  ', body);
    fireEvent.click(screen.getByRole('button', { name: L.retract }));

    // The marked sentence is already in the field, whitespace collapsed.
    expect(screen.getByDisplayValue('Yassine is een vriend van je')).toBeTruthy();

    fireEvent.change(screen.getByLabelText(L.retractCorrectionLabel), { target: { value: '  Yassine is een kennis  ' } });
    fireEvent.click(screen.getByText(L.retractSend));

    await waitFor(() => expect(calls).toEqual([['Yassine is een vriend van je', 'Yassine is een kennis']]));
    // The answer stays under the message, in the server's own words.
    expect(screen.getByText('Taken back: "Yassine is een vriend van je".')).toBeTruthy();
  });

  it('ignores a highlight outside her reply, so a stray selection never becomes the quote', () => {
    render(<MessageView message={reply()} onRetract={async () => ({ status: 'not_found', message: 'Niets.' })} />);
    selectionAt('iets heel anders', document.body);

    fireEvent.click(screen.getByRole('button', { name: L.retract }));

    const field = screen.getByLabelText(L.retractQuoteLabel);
    expect(field.value).toBe('');
    expect(screen.getByText(L.retractSend).disabled).toBe(true);
  });

  it('offers the memories it found when more than one fits, and takes back the one chosen', async () => {
    const calls = [];
    const onRetract = async (quote) => {
      calls.push(quote);
      if (calls.length === 1) {
        return {
          status: 'ambiguous',
          message: 'More than one memory matches.',
          candidates: [
            { bank: 'bojan', id: 'f1', text: 'Yassine is een vriend van je' },
            { bank: 'gaia-logos', id: 'f2', text: 'Yassine is een vriend van je' },
          ],
        };
      }
      return { status: 'retracted', message: 'Taken back.' };
    };
    render(<MessageView message={reply()} onRetract={onRetract} />);

    fireEvent.click(screen.getByRole('button', { name: L.retract }));
    fireEvent.change(screen.getByLabelText(L.retractQuoteLabel), { target: { value: 'Yassine is een vriend van je' } });
    fireEvent.click(screen.getByText(L.retractSend));

    await waitFor(() => expect(screen.getByText(L.retractPick)).toBeTruthy());
    fireEvent.click(screen.getAllByText('Yassine is een vriend van je')[1]);

    await waitFor(() => expect(calls).toEqual(['Yassine is een vriend van je', 'Yassine is een vriend van je']));
    expect(screen.getByText('Taken back.')).toBeTruthy();
  });

  it('says it could not take it back rather than showing a raw failure', async () => {
    const onRetract = async () => { throw new Error('gaia server responded 500'); };
    render(<MessageView message={reply()} onRetract={onRetract} />);

    fireEvent.click(screen.getByRole('button', { name: L.retract }));
    fireEvent.change(screen.getByLabelText(L.retractQuoteLabel), { target: { value: 'iets' } });
    fireEvent.click(screen.getByText(L.retractSend));

    await waitFor(() => expect(screen.getByText(L.retractFailed)).toBeTruthy());
  });
});
