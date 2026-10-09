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
    // Only role/content cross the seam, plus the one field the server needs
    // to keep a readable chat log: the turn's own time (`createdAt`). Absent
    // on messages that predate timestamps, in which case it's simply left
    // off the envelope rather than sent as null.
    messages: messages.map(({ role, content, createdAt }) => {
      const message = { role, content };
      if (createdAt) message.createdAt = createdAt;
      return message;
    }),
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

// --- cognition review (cognition/UnderstandingRail.jsx) ---------------------
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
 * The human reopen — the only way out of the rejected quarantine. `rejected`
 * is terminal for every automatic path; the reason is required.
 */
export function buildCognitionReopenRequest(id, reason) {
  return { method: 'post', path: `cognition/hypotheses/${id}/reopen`, body: { reason } };
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

// --- Kairos episodes (logos/EpisodeTimeline.jsx) ----------------------------
// Episodes Logos has recognised: a time block it read as one coherent activity.
// Each carries its own interpretation plus the raw captures it was drawn from,
// so the UI can always step back from interpretation to observation. Plain JSON
// over the same generic server_request seam — the card separates the two; this
// seam only carries them.
//
// Server side is the Kairos pipeline (gaia-api): GET /kairos/episodes returns
// { data, pagination } of derived episodes, each `epistemic_status:
// 'interpretation'` with `sources` pointing back to the raw observations. The
// card wants a different shape (title/interpretation/context/observations), so
// adaptKairosEpisode is the one translation point — the card is never made to
// know the server's field names.

/** The episodes Kairos has synthesised, newest first. */
export function buildEpisodeListRequest(query = {}) {
  const params = new URLSearchParams();
  if (query.page) params.set('page', String(query.page));
  if (query.limit) params.set('limit', String(query.limit));
  if (query.since) params.set('since', query.since);
  const suffix = params.toString() ? `?${params}` : '';
  return { method: 'get', path: `kairos/episodes${suffix}` };
}

/**
 * The raw observations behind one episode — the audit path from interpretation
 * down to evidence. Reads Kairos's own proxy (GET /kairos/episodes/:id/evidence),
 * never Foundation directly.
 */
export function buildEpisodeEvidenceRequest(id) {
  return { method: 'get', path: `kairos/episodes/${encodeURIComponent(id)}/evidence` };
}

/**
 * Maps one Kairos episode (server shape) to the shape EpisodeCard renders.
 * Only fields we actually have are populated: no confidence score, no session —
 * an honest absence beats a fabricated number. The evidence is left empty here
 * and loaded on demand by the card, so the list stays one cheap call.
 */
export function adaptKairosEpisode(episode) {
  if (!episode || typeof episode !== 'object') return null;
  const apps = Array.isArray(episode.involved_apps) ? episode.involved_apps : [];
  const primary = episode.primary_app || apps[0] || '';
  return {
    id: episode.id,
    startTime: episode.start_time,
    endTime: episode.end_time,
    title: primary || null,
    interpretation: episode.summary || '',
    epistemicStatus: episode.epistemic_status || 'interpretation',
    context: apps.map((application) => ({ application, type: 'application' })),
    sources: Array.isArray(episode.sources) ? episode.sources : [],
    observations: [],
  };
}

/**
 * Normalizes the episodes envelope. Defensive on shape because this crosses a
 * seam outside this repo: a server that predates the endpoint degrades to the
 * empty state rather than crashing. Accepts both the Kairos `{ data: [...] }`
 * envelope and a bare array.
 */
export function parseEpisodeList(response) {
  const body = response?.body;
  const raw = Array.isArray(body) ? body : body?.data;
  if (!Array.isArray(raw)) return [];
  return raw.map(adaptKairosEpisode).filter(Boolean);
}

/**
 * Normalizes the evidence envelope: `{ episode, observations }` where each
 * observation is Foundation's raw ingest object (`{ object, episodes }`).
 * Maps to the fields EpisodeCard's evidence rows read.
 */
export function parseEpisodeEvidence(response) {
  const observations = response?.body?.observations;
  if (!Array.isArray(observations)) return [];
  return observations.map((entry) => {
    const object = entry?.object || {};
    const tags = Array.isArray(object.tags) ? object.tags : [];
    return {
      id: object.id || entry?.id || '',
      timestamp: object.occurred_at || object.ingested_at || null,
      application: tags[0] || null,
      windowTitle: object.title || null,
      ocrText: object.content || '',
      uia: null,
      screenshot: null,
    };
  });
}
