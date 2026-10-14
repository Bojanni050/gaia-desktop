import React from 'react';
import { describe, it, expect, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';

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
