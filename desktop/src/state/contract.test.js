import { describe, it, expect } from 'vitest';
import { buildTurnRequest, parseReply } from './contract';

describe('buildTurnRequest', () => {
  it('wraps message history in a conversation/turn envelope', () => {
    const request = buildTurnRequest([
      { id: '1', role: 'user', content: 'hello' },
      { id: '2', role: 'assistant', content: 'hi' },
    ]);

    expect(request.method).toBe('post');
    expect(request.path).toBe('conversation/turn');
    expect(request.body.messages).toEqual([
      { role: 'user', content: 'hello' },
      { role: 'assistant', content: 'hi' },
    ]);
  });

  it('sends only role and content — no local ids or ui state leak to the server', () => {
    const request = buildTurnRequest([{ id: 'x', role: 'user', content: 'hi', failed: false }]);
    expect(Object.keys(request.body.messages[0])).toEqual(['role', 'content']);
  });

  it('includes attachmentIds when the latest user turn has attachments', () => {
    const request = buildTurnRequest([
      { id: '1', role: 'user', content: 'what does this say?', attachments: [{ id: 'f1', filename: 'a.txt' }], attachmentIds: ['f1'] },
    ]);
    expect(request.body.attachmentIds).toEqual(['f1']);
    // still only role/content per message — attachment data never leaks into the message shape
    expect(Object.keys(request.body.messages[0])).toEqual(['role', 'content']);
  });

  it('omits attachmentIds entirely when there are none', () => {
    const request = buildTurnRequest([{ id: '1', role: 'user', content: 'hi' }]);
    expect(request.body.attachmentIds).toBeUndefined();
  });

  it('never carries attachmentIds from an earlier turn forward onto a later one', () => {
    const request = buildTurnRequest([
      { id: '1', role: 'user', content: 'first', attachmentIds: ['f1'] },
      { id: '2', role: 'assistant', content: 'reply' },
      { id: '3', role: 'user', content: 'second, no attachment this time' },
    ]);
    expect(request.body.attachmentIds).toBeUndefined();
  });
});

describe('parseReply', () => {
  it('reads the reply string from the response body', () => {
    expect(parseReply({ status: 200, body: { reply: 'hi' } })).toBe('hi');
  });

  it('rejects a body without a reply', () => {
    expect(() => parseReply({ status: 200, body: {} })).toThrow();
  });

  it('rejects an empty reply', () => {
    expect(() => parseReply({ status: 200, body: { reply: '' } })).toThrow();
  });
});
