/**
 * The conversation contract between Gaia Desktop and Gaia Cloud.
 *
 * The desktop sends plain user turns and renders plain replies. Identity,
 * memory, intent and reasoning all happen server-side: this file declares
 * the envelope, nothing more. When the cloud-side endpoint grows streaming,
 * only the transport beneath this seam changes.
 */
/**
 * @param {Array} messages
 * @param {string} [conversationId] the thread's own local id — carried
 *   through so the server can save/append the transcript under it
 *   (conversationStore.js). Chat history, never Hindsight: the raw log a
 *   person reopens, not a reflective memory. Omit it for a turn that
 *   shouldn't be saved (there currently isn't one, but the contract
 *   doesn't require it).
 */
function buildTurnBody(messages, conversationId) {
  const body = {
    messages: messages.map(({ role, content }) => ({ role, content })),
  };
  if (conversationId) body.conversationId = conversationId;
  // Attachments belong to whichever user turn just triggered this request —
  // never re-sent for older turns already in history, and never carrying
  // the file bytes themselves (those already live in the library from
  // upload; only their ids cross this seam).
  const last = messages[messages.length - 1];
  if (last && last.role === 'user' && Array.isArray(last.attachmentIds) && last.attachmentIds.length > 0) {
    body.attachmentIds = last.attachmentIds;
  }
  return body;
}

export function buildTurnRequest(messages, conversationId) {
  return { method: 'post', path: 'conversation/turn', body: buildTurnBody(messages, conversationId) };
}

/**
 * The streaming path's body — same shape as buildTurnRequest's, minus the
 * ServerRequest envelope (server_stream_turn posts straight to
 * `conversation/turn`, there's no method/path to carry). Note:
 * attachmentIds is included for shape-consistency but currently has no
 * effect server-side — performStreamingTurn doesn't resolve attachments
 * into the prompt the way the non-streaming path does (gaia-api/src/
 * server.js); a pre-existing gap shared with Web, not introduced here.
 */
export function buildStreamTurnBody(messages, conversationId) {
  return buildTurnBody(messages, conversationId);
}

export function parseReply(response) {
  const reply = response?.body?.reply;
  if (typeof reply === 'string' && reply.length > 0) {
    return reply;
  }
  throw new Error('Gaia Server returned no reply');
}

// --- chat history (history/HistorySection.jsx) -----------------------------
// Plain JSON, reached through the same generic server_request seam as a
// turn — unlike the library, nothing here is a file upload.

export function buildHistoryListRequest() {
  return { method: 'get', path: 'conversations' };
}

export function buildHistoryGetRequest(id) {
  return { method: 'get', path: `conversations/${id}` };
}

export function buildHistoryDeleteRequest(id) {
  return { method: 'delete', path: `conversations/${id}` };
}

export function buildHistoryExportJsonRequest(id) {
  return { method: 'get', path: `conversations/${id}/export/json` };
}

export function buildHistoryExportMarkdownRequest(id) {
  return { method: 'get', path: `conversations/${id}/export/markdown` };
}

export function parseHistoryList(response) {
  const conversations = response?.body?.conversations;
  return Array.isArray(conversations) ? conversations : [];
}

export function parseHistoryConversation(response) {
  const messages = response?.body?.messages;
  if (!Array.isArray(messages)) {
    throw new Error('Gaia Server returned no conversation');
  }
  return { meta: response.body.meta || {}, messages };
}

export function parseHistoryExport(response, format) {
  const body = response?.body;
  if (!body) {
    throw new Error('Gaia Server returned no export data');
  }

  if (format === 'json') {
    const conversation = body.conversation;
    if (!conversation) {
      throw new Error('Gaia Server returned invalid export data');
    }
    return JSON.stringify(body, null, 2);
  }

  if (format === 'markdown') {
    if (typeof body !== 'string') {
      throw new Error('Gaia Server returned invalid markdown export');
    }
    return body;
  }

  throw new Error(`Unknown export format: ${format}`);
}
