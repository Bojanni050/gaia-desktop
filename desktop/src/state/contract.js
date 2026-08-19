/**
 * The conversation contract between Gaia Desktop and Gaia Cloud.
 *
 * The desktop sends plain user turns and renders plain replies. Identity,
 * memory, intent and reasoning all happen server-side: this file declares
 * the envelope, nothing more. When the cloud-side endpoint grows streaming,
 * only the transport beneath this seam changes.
 */
export function buildTurnRequest(messages) {
  return {
    method: 'post',
    path: 'conversation/turn',
    body: {
      messages: messages.map(({ role, content }) => ({ role, content })),
    },
  };
}

export function parseReply(response) {
  const reply = response?.body?.reply;
  if (typeof reply === 'string' && reply.length > 0) {
    return reply;
  }
  throw new Error('Gaia Server returned no reply');
}
