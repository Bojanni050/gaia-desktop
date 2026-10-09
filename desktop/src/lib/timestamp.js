/**
 * "03-10-2026 14:32" — the date and time a chat turn was said. Wall-clock
 * and deliberately locale-free, the same reasoning EpisodeCard's own clock
 * uses: the label is read for orientation, never sorted or parsed, so a
 * fixed dd-mm-yyyy HH:MM shape reads the same in every environment instead
 * of shifting with the machine's locale. Returns null for anything
 * unparseable, so the caller renders nothing rather than "Invalid Date".
 */
const pad = (n) => String(n).padStart(2, '0');

export function formatTurnTime(iso) {
  if (!iso) return null;
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return null;
  const day = pad(date.getDate());
  const month = pad(date.getMonth() + 1);
  const hours = pad(date.getHours());
  const minutes = pad(date.getMinutes());
  return `${day}-${month}-${date.getFullYear()} ${hours}:${minutes}`;
}
