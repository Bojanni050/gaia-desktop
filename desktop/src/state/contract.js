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
export function buildTurnRequest(messages, conversationId) {
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
  return { method: 'post', path: 'conversation/turn', body };
}

export function parseReply(response) {
  const reply = response?.body?.reply;
  if (typeof reply === 'string' && reply.length > 0) {
    return reply;
  }
  throw new Error('Gaia Server returned no reply');
}

// --- chat history (history/HistoryPanel.jsx) ------------------------------
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
