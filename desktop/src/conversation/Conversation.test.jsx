import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import React from 'react';
import { render, cleanup, fireEvent } from '@testing-library/react';

// The Conversation renders the Composer, which reaches the Rust shell for
// push-to-talk and speech; nothing here may touch a real Tauri command.
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({})) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));
vi.mock('../lib/speech', () => ({ playSpeech: vi.fn(async () => {}), stopSpeech: vi.fn(), speaksReplies: vi.fn(() => true) }));
vi.mock('../state/usePushToTalk', () => ({
  usePushToTalk: () => ({ phase: 'idle', error: null, available: false, micOn: false, toggle: vi.fn(), cancel: vi.fn() }),
}));

import Conversation from './Conversation';

afterEach(cleanup);

// jsdom lays nothing out, so the scrolling container's metrics are the test's
// own: one shared stand-in for the browser's layout, driven per test.
const view = { content: 0, viewport: 600, top: 0 };
let jumps = [];

beforeEach(() => {
  view.content = 1200;
  view.viewport = 600;
  view.top = view.content - view.viewport;
  jumps = [];
  Object.defineProperty(HTMLElement.prototype, 'scrollHeight', { configurable: true, get: () => view.content });
  Object.defineProperty(HTMLElement.prototype, 'clientHeight', { configurable: true, get: () => view.viewport });
  Object.defineProperty(HTMLElement.prototype, 'scrollTop', {
    configurable: true,
    get: () => view.top,
    // The browser clamps a jump to the scrollable range; so does this.
    set: (value) => { view.top = Math.max(0, Math.min(value, view.content - view.viewport)); },
  });
  HTMLElement.prototype.scrollTo = function (options) {
    jumps.push(typeof options === 'number' ? { top: options } : options);
    this.scrollTop = typeof options === 'number' ? options : options.top;
  };
});

const bottom = () => view.content - view.viewport;

const thread = (messages) => ({ id: 't1', title: null, messages });
const asked = { id: 'u1', role: 'user', content: 'waarom is de vps onbereikbaar?', createdAt: null };
const reply = (content) => ({ id: 'a1', role: 'assistant', content, createdAt: null });

const props = (overrides = {}) => ({
  busy: true,
  streaming: true,
  progress: null,
  presenceState: 'quiet',
  listenState: 'off',
  whisper: null,
  onSend: () => {},
  onRetry: () => {},
  research: null,
  onOpenResearchConversation: () => {},
  desktop: null,
  ...overrides,
});

/** Renders one thread and hands back its scrolling container. */
const show = (messages, overrides = {}) => {
  const utils = render(<Conversation thread={thread(messages)} {...props(overrides)} />);
  return { ...utils, el: utils.container.querySelector('.conversation-scroll') };
};

/** The reply grew: more text is laid out, the person touched nothing. */
const replyGrows = (px) => { view.content += px; };

/** The person drags the view up and the browser reports it. */
const personScrollsUp = (px, el) => { view.top -= px; fireEvent.scroll(el); };

describe('following a reply while it streams', () => {
  it('stays at the tail when the reply grows faster than the browser reports our own jump', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    expect(el.scrollTop).toBe(bottom());

    // The reply grows and only THEN is the scroll event of our own jump
    // delivered — the growth reads as distance-from-bottom although nobody
    // scrolled. This is what used to stop the view mid-answer.
    replyGrows(300);
    fireEvent.scroll(el);

    rerender(<Conversation thread={thread([asked, reply('Eerste deel. En nog een stuk dat eraan groeit.')])} {...props()} />);
    expect(el.scrollTop).toBe(bottom());
  });

  it('keeps following after a small growth the person could not even see', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    replyGrows(50);
    fireEvent.scroll(el);
    rerender(<Conversation thread={thread([asked, reply('Eerste deel. Nog eentje.')])} {...props()} />);
    expect(el.scrollTop).toBe(bottom());
  });

  it('never animates a jump: the reply is written faster than an animation could follow', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    rerender(<Conversation thread={thread([asked, reply('Eerste deel, en nog een regel.')])} {...props()} />);
    expect(jumps.length).toBeGreaterThan(0);
    expect(jumps.every((jump) => jump.behavior === 'auto')).toBe(true);
  });

  it('leaves the view alone once the person scrolls up themselves', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    personScrollsUp(400, el);
    const whereThePersonIs = el.scrollTop;

    replyGrows(200);
    rerender(<Conversation thread={thread([asked, reply('Eerste deel. En de rest van het antwoord.')])} {...props()} />);
    expect(el.scrollTop).toBe(whereThePersonIs);
  });

  it('follows again when the person scrolls back to the tail', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    personScrollsUp(400, el);
    el.scrollTop = el.scrollHeight - el.clientHeight;
    fireEvent.scroll(el);

    replyGrows(200);
    rerender(<Conversation thread={thread([asked, reply('Eerste deel. En de rest van het antwoord.')])} {...props()} />);
    expect(el.scrollTop).toBe(bottom());
  });

  it('brings the tail back into view when the person sends a new message', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    personScrollsUp(400, el);

    rerender(<Conversation thread={thread([asked, reply('Eerste deel.'), { id: 'u2', role: 'user', content: 'en nog iets', createdAt: null }])} {...props()} />);
    expect(el.scrollTop).toBe(bottom());
  });

  it('opens another conversation at its newest turn', () => {
    const { el, rerender } = show([asked, reply('Eerste deel.')]);
    view.content = 4000;
    personScrollsUp(200, el);

    rerender(<Conversation thread={{ id: 't2', title: null, messages: [asked, reply('Een ander gesprek.')] }} {...props()} />);
    expect(el.scrollTop).toBe(bottom());
  });
});
