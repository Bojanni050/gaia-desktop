/**
 * The conversation contract between Gaia Desktop and Gaia Cloud.
 *
 * The desktop sends plain user turns and renders plain replies. Identity,
 * memory, intent and reasoning all happen server-side: this file declares
 * the envelope, nothing more. When the cloud-side endpoint grows streaming,
 * only the transport beneath this seam changes.
 */
export function buildTurnRequest(messages) {
  const body = {
    messages: messages.map(({ role, content }) => ({ role, content })),
  };
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
