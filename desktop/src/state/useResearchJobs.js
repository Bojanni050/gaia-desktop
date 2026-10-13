/**
 * The background research Gaia put out at Hermes.
 *
 * A research turn is answered immediately; the findings arrive later, written
 * into the conversation by the server. So the UI cannot learn about them from
 * the turn — it asks. This hook is that ask: one small poll, the three lists it
 * returns (running, awaiting a decision, finished but unseen), and the two
 * things a person can do about them.
 *
 * A poll, not the realtime stream, on purpose: a run can outlive the window, a
 * reconnect, or the app itself, and only a durable ask survives that. Every
 * failure is silent — a failed poll keeps what we last knew rather than
 * blanking the UI.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import {
  buildResearchJobsRequest,
  buildResearchAckRequest,
  buildResearchApprovalRequest,
  parseResearchJobs,
} from './contract';

/**
 * @param {object} server the serverApi seam
 * @param {{ onFinished?: (job: object) => void, intervalMs?: number }} [options]
 *   `onFinished` fires ONCE per finished job, the first time it is seen — the
 *   delivery seam. The caller fetches the conversation the findings were
 *   written into, then acknowledges the job.
 */
export function useResearchJobs(server, { onFinished, intervalMs = 8000 } = {}) {
  const [running, setRunning] = useState([]);
  const [awaiting, setAwaiting] = useState([]);
  const [finished, setFinished] = useState([]);

  const onFinishedRef = useRef(onFinished);
  onFinishedRef.current = onFinished;
  // Jobs whose delivery already ran, so a poll that keeps returning the same
  // finished job (until it is acknowledged) does not fetch the conversation
  // over and over.
  const handledRef = useRef(new Set());

  const refresh = useCallback(async () => {
    try {
      const jobs = parseResearchJobs(await server.request(buildResearchJobsRequest()));
      setRunning(jobs.running);
      setAwaiting(jobs.awaiting);
      setFinished(jobs.finished);
      return jobs;
    } catch (_) {
      return null; // keep the last known state
    }
  }, [server]);

  useEffect(() => {
    let alive = true;
    const tick = async () => {
      const jobs = await refresh();
      if (!alive || !jobs) return;
      for (const job of jobs.finished) {
        if (handledRef.current.has(job.id)) continue;
        handledRef.current.add(job.id);
        try {
          if (onFinishedRef.current) await onFinishedRef.current(job);
        } catch (_) {
          // Delivery failing must not stop the poll; the job stays unseen.
          handledRef.current.delete(job.id);
        }
      }
    };
    tick();
    const timer = setInterval(tick, intervalMs);
    return () => {
      alive = false;
      clearInterval(timer);
    };
  }, [refresh, intervalMs]);

  /** Answer a run that parked on a command: one of the job's own choices. */
  const answer = useCallback(
    async (id, choice) => {
      await server.request(buildResearchApprovalRequest(id, choice));
      await refresh();
    },
    [server, refresh]
  );

  /** Mark a finished job as seen. */
  const ack = useCallback(
    async (id) => {
      await server.request(buildResearchAckRequest(id));
      setFinished((prev) => prev.filter((job) => job.id !== id));
    },
    [server]
  );

  return { running, awaiting, finished, refresh, answer, ack };
}
