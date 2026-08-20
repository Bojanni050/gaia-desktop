import { describe, it, expect } from 'vitest';
import {
  buildTurnRequest,
  buildStreamTurnBody,
  parseReply,
  buildHistoryListRequest,
  buildHistoryGetRequest,
  buildHistoryDeleteRequest,
  parseHistoryList,
  parseHistoryConversation,
} from './contract';

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

  it('includes conversationId when given', () => {
    const request = buildTurnRequest([{ id: '1', role: 'user', content: 'hi' }], 'thread-42');
    expect(request.body.conversationId).toBe('thread-42');
  });

  it('omits conversationId when not given', () => {
    const request = buildTurnRequest([{ id: '1', role: 'user', content: 'hi' }]);
    expect(request.body.conversationId).toBeUndefined();
  });
});

describe('buildStreamTurnBody', () => {
  it('produces the same body shape as buildTurnRequest, with no method/path envelope', () => {
    const messages = [
      { id: '1', role: 'user', content: 'hello', attachmentIds: ['f1'] },
      { id: '2', role: 'assistant', content: 'hi' },
    ];
    const request = buildTurnRequest(messages, 'thread-1');
    const streamBody = buildStreamTurnBody(messages, 'thread-1');
    expect(streamBody).toEqual(request.body);
  });

  it('includes conversationId and attachmentIds under the same rules as buildTurnRequest', () => {
    const body = buildStreamTurnBody(
      [{ id: '1', role: 'user', content: 'hi', attachmentIds: ['f1'] }],
      'thread-9'
    );
    expect(body.conversationId).toBe('thread-9');
    expect(body.attachmentIds).toEqual(['f1']);
    expect(Object.keys(body.messages[0])).toEqual(['role', 'content']);
  });
});

describe('history contract', () => {
  it('buildHistoryListRequest is a plain GET, no body', () => {
    expect(buildHistoryListRequest()).toEqual({ method: 'get', path: 'conversations' });
  });

  it('buildHistoryGetRequest addresses one conversation by id', () => {
    expect(buildHistoryGetRequest('conv-1')).toEqual({ method: 'get', path: 'conversations/conv-1' });
  });

  it('buildHistoryDeleteRequest is a DELETE to the same path', () => {
    expect(buildHistoryDeleteRequest('conv-1')).toEqual({ method: 'delete', path: 'conversations/conv-1' });
  });

  it('parseHistoryList reads the conversations array', () => {
    const list = parseHistoryList({ body: { conversations: [{ id: 'a' }] } });
    expect(list).toEqual([{ id: 'a' }]);
  });

  it('parseHistoryList defaults to [] for a malformed response', () => {
    expect(parseHistoryList({ body: {} })).toEqual([]);
    expect(parseHistoryList({})).toEqual([]);
  });

  it('parseHistoryConversation reads meta and messages', () => {
    const result = parseHistoryConversation({ body: { meta: { id: 'a' }, messages: [{ role: 'user', content: 'hi' }] } });
    expect(result.meta).toEqual({ id: 'a' });
    expect(result.messages).toEqual([{ role: 'user', content: 'hi' }]);
  });

  it('parseHistoryConversation throws when messages is missing', () => {
    expect(() => parseHistoryConversation({ body: {} })).toThrow();
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
