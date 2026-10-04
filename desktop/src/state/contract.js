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

// --- cognition review (cognition/ReviewSection.jsx) -------------------------
// The human Absolute Override. Derived statements Logos is still weighing live
// server-side; the person lists them and moves one forward or lets it go.
// Plain JSON over the same generic server_request seam — never a file.

/** The pending/testing statements awaiting review. */
export function buildCognitionListRequest() {
  return { method: 'get', path: 'cognition/hypotheses' };
}

export function buildCognitionTestRequest(id) {
  return { method: 'post', path: `cognition/hypotheses/${id}/test` };
}

export function buildCognitionRejectRequest(id, reason) {
  const request = { method: 'post', path: `cognition/hypotheses/${id}/reject` };
  if (reason) request.body = { reason };
  return request;
}

/**
 * `confirm` is the only path to `confirmed`. `supersedes` names the older,
 * contradicting statements this confirmation replaces (the server marks them
 * rejected as `consolidatie`); `rationale` is the human-readable "why now".
 */
export function buildCognitionConfirmRequest(id, { supersedes, rationale, statement } = {}) {
  const request = { method: 'post', path: `cognition/hypotheses/${id}/confirm` };
  const body = {};
  if (Array.isArray(supersedes) && supersedes.length > 0) body.supersedes = supersedes;
  if (rationale) body.rationale = rationale;
  // `statement` is the human's own nuanced re-wording ("Nuanceren"); the
  // server writes it together with the confirmation as one audited act.
  if (statement) body.statement = statement;
  if (Object.keys(body).length > 0) request.body = body;
  return request;
}

export function parseCognitionList(response) {
  const hypotheses = response?.body?.hypotheses;
  return Array.isArray(hypotheses) ? hypotheses : [];
}
