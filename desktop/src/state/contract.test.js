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
  buildResearchJobsRequest,
  parseResearchJobs,
  buildResearchAckRequest,
  buildResearchApprovalRequest,
  parseResearchApproval,
  parseTurnResearch,
  buildCognitionListRequest,
  buildCognitionTestRequest,
  buildCognitionRejectRequest,
  buildCognitionReopenRequest,
  buildCognitionAnswerRequest,
  buildCognitionConfirmRequest,
  parseCognitionList,
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

  it('carries the turn time when a message has one, and nothing else', () => {
    const request = buildTurnRequest([
      { id: 'x', role: 'user', content: 'hi', createdAt: '2026-10-03T10:15:00.000Z', failed: false },
    ]);
    expect(request.body.messages[0]).toEqual({
      role: 'user',
      content: 'hi',
      createdAt: '2026-10-03T10:15:00.000Z',
    });
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

describe('buildStreamTurnBody desktopRoots', () => {
  const messages = [{ id: '1', role: 'user', content: 'hi' }];

  it('sends the allowed folders only together with desktop: true', () => {
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopRoots: ['D:/'] }).desktopRoots).toEqual(['D:/']);
    expect(buildStreamTurnBody(messages, 't', { desktop: false, desktopRoots: ['D:/'] })).not.toHaveProperty('desktopRoots');
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopRoots: [] })).not.toHaveProperty('desktopRoots');
    expect(buildStreamTurnBody(messages, 't', { desktop: true })).not.toHaveProperty('desktopRoots');
  });

  it('says the search index is there only when it is, and only together with desktop: true', () => {
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopIndex: true }).desktopIndex).toBe(true);
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopIndex: false })).not.toHaveProperty('desktopIndex');
    expect(buildStreamTurnBody(messages, 't', { desktop: true })).not.toHaveProperty('desktopIndex');
    expect(buildStreamTurnBody(messages, 't', { desktop: false, desktopIndex: true })).not.toHaveProperty('desktopIndex');
  });

  it('sends the known places only together with desktop: true', () => {
    const places = { desktop: 'D:/OneDrive/Bureaublad' };
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopPlaces: places }).desktopPlaces).toEqual(places);
    expect(buildStreamTurnBody(messages, 't', { desktop: false, desktopPlaces: places })).not.toHaveProperty('desktopPlaces');
    expect(buildStreamTurnBody(messages, 't', { desktop: true, desktopPlaces: {} })).not.toHaveProperty('desktopPlaces');
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

describe('cognition contract', () => {
  it('buildCognitionListRequest is a plain GET', () => {
    expect(buildCognitionListRequest()).toEqual({ method: 'get', path: 'cognition/hypotheses' });
  });

  it('buildCognitionTestRequest addresses one statement', () => {
    expect(buildCognitionTestRequest('h1')).toEqual({ method: 'post', path: 'cognition/hypotheses/h1/test' });
  });

  it('buildCognitionRejectRequest carries a reason only when given', () => {
    expect(buildCognitionRejectRequest('h1')).toEqual({ method: 'post', path: 'cognition/hypotheses/h1/reject' });
    expect(buildCognitionRejectRequest('h1', 'not true')).toEqual({
      method: 'post', path: 'cognition/hypotheses/h1/reject', body: { reason: 'not true' },
    });
  });

  it('buildCognitionReopenRequest always carries the reason', () => {
    expect(buildCognitionReopenRequest('h1', 'the disproof was retracted')).toEqual({
      method: 'post', path: 'cognition/hypotheses/h1/reopen', body: { reason: 'the disproof was retracted' },
    });
  });

  it('buildCognitionAnswerRequest always carries the answer', () => {
    expect(buildCognitionAnswerRequest('q1', 'inhouden')).toEqual({
      method: 'post', path: 'cognition/hypotheses/q1/answer', body: { answer: 'inhouden' },
    });
  });

  it('buildCognitionConfirmRequest omits an empty body and includes supersedes/rationale/statement', () => {
    expect(buildCognitionConfirmRequest('h1')).toEqual({ method: 'post', path: 'cognition/hypotheses/h1/confirm' });
    expect(buildCognitionConfirmRequest('h2', { supersedes: ['h1'], rationale: 'newer value' })).toEqual({
      method: 'post', path: 'cognition/hypotheses/h2/confirm', body: { supersedes: ['h1'], rationale: 'newer value' },
    });
    expect(buildCognitionConfirmRequest('h2', { rationale: 'nuanced', statement: 'my own wording' })).toEqual({
      method: 'post', path: 'cognition/hypotheses/h2/confirm', body: { rationale: 'nuanced', statement: 'my own wording' },
    });
  });

  it('parseCognitionList reads the hypotheses array and defaults to []', () => {
    expect(parseCognitionList({ body: { hypotheses: [{ id: 'h1' }] } })).toEqual([{ id: 'h1' }]);
    expect(parseCognitionList({ body: {} })).toEqual([]);
    expect(parseCognitionList({})).toEqual([]);
  });
});

// --- research jobs ---------------------------------------------------------

describe('research job seam', () => {
  it('asks for the queue over the generic server_request seam', () => {
    expect(buildResearchJobsRequest()).toEqual({ method: 'get', path: 'research/jobs' });
  });

  it('keeps the three lists apart, and tolerates a malformed body', () => {
    const parsed = parseResearchJobs({
      body: { running: [{ id: 'rj_1' }], awaiting: [{ id: 'rj_2' }], finished: [{ id: 'rj_3' }] },
    });
    expect(parsed.running).toHaveLength(1);
    expect(parsed.awaiting).toHaveLength(1);
    expect(parsed.finished).toHaveLength(1);

    const empty = parseResearchJobs(undefined);
    expect(empty).toEqual({ running: [], awaiting: [], finished: [] });
  });

  it('acknowledges and answers one job, encoding the id', () => {
    expect(buildResearchAckRequest('rj 1')).toEqual({ method: 'post', path: 'research/jobs/rj%201/ack' });
    expect(buildResearchApprovalRequest('rj_1', 'deny')).toEqual({
      method: 'post',
      path: 'research/jobs/rj_1/approval',
      body: { choice: 'deny' },
    });
  });

  it('reads the question out of a job, and finds none where there is none', () => {
    expect(
      parseResearchApproval({ approval: { command: 'python3 x.py', description: 'schrijft weg', choices: ['once', 'deny'] } })
    ).toEqual({ command: 'python3 x.py', description: 'schrijft weg', choices: ['once', 'deny'] });

    expect(parseResearchApproval({ status: 'running' })).toBeNull();
    expect(parseResearchApproval({ approval: { description: 'geen commando' } })).toBeNull();
    // A job that offers no choices still has the safe pair.
    expect(parseResearchApproval({ approval: { command: 'ls' } }).choices).toEqual(['once', 'deny']);
  });

  it('reads the research job a turn started, and nothing when there is none', () => {
    expect(parseTurnResearch({ body: { research: { id: 'rj_1', status: 'running' } } }).id).toBe('rj_1');
    expect(parseTurnResearch({ body: { reply: 'hoi' } })).toBeNull();
  });
});
