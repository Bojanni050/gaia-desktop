/**
 * Plan progress → the words the thinking row shows while a multi-step plan
 * is still running (Gaia Server's Decision Engine composes plans; its
 * responseEngine sends each step's progress as a `step` frame; THIS module
 * only decides how that fact reads to a person).
 *
 * A frame names a plan step in the Decision Engine's own vocabulary —
 * retrieval / reasoning / generation / capability — and never the
 * capability behind it, so the wording here is activity, not machinery:
 * "searching", not "calling conversation_search". Presentation only; it
 * can never change what Gaia is doing or what her answer will be.
 */

const ACTIVITY = {
  retrieval: 'searching',
  reasoning: 'thinking it through',
  generation: 'answering',
  capability: 'looking it up',
};

/**
 * @param {{ index?: number, total?: number, type?: string }|null} progress
 *   the `step` payload of a `{ status: 'start' }` frame
 * @returns {string|null} e.g. "Step 2 of 3 - searching...", or null when
 *   there is no progress to show (the sequenced thinking message then
 *   remains the honest state).
 */
export function stepProgressLabel(progress) {
  if (!progress || typeof progress.index !== 'number' || typeof progress.total !== 'number') {
    return null;
  }
  const activity = ACTIVITY[progress.type] || 'working';
  return `Step ${progress.index} of ${progress.total} - ${activity}...`;
}
