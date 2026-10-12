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
