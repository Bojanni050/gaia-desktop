/**
 * Calm phrasing for failed turns. The desktop never shows HTTP codes,
 * provider names or stack traces — only quiet, human sentences.
 */
export function phraseTurnError(error) {
  const kind = error?.kind;
  const message = String(error?.message || '');

  if (kind === 'communication') {
    if (message.includes('no Gaia Server configured')) {
      return 'Gaia is not connected to her server yet. You can add one in settings.';
    }
    if (message.includes('server responded')) {
      return 'Gaia could not answer right now. She will be back.';
    }
    return 'Gaia cannot be reached right now.';
  }
  if (kind === 'capture') {
    return 'This device could not capture that.';
  }
  return 'Something went wrong on this side. Your message is still here.';
}
