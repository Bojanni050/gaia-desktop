import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useResearchJobs } from './useResearchJobs';
import {
  buildResearchJobsRequest,
  buildResearchAckRequest,
  buildResearchApprovalRequest,
} from './contract';

/** A server stub that answers the queue with whatever the test set. */
function fakeServer(jobs = { running: [], awaiting: [], finished: [] }) {
  const calls = [];
  return {
    calls,
    jobs,
    request: vi.fn(async (request) => {
      calls.push(request);
      if (request.path === 'research/jobs') return { body: jobs };
      return { body: {} };
    }),
  };
}

beforeEach(() => vi.useFakeTimers({ shouldAdvanceTime: true }));
afterEach(() => vi.useRealTimers());

describe('useResearchJobs', () => {
  it('asks the queue on mount and keeps the three lists apart', async () => {
    const server = fakeServer({
      running: [{ id: 'rj_run' }],
      awaiting: [{ id: 'rj_ask' }],
      finished: [],
    });

    const { result } = renderHook(() => useResearchJobs(server));

    await waitFor(() => expect(result.current.running).toHaveLength(1));
    expect(result.current.awaiting[0].id).toBe('rj_ask');
    expect(server.calls[0]).toEqual(buildResearchJobsRequest());
  });

  it('delivers a finished job exactly once, however often it polls', async () => {
    const server = fakeServer({ running: [], awaiting: [], finished: [{ id: 'rj_done', conversationId: 'c1' }] });
    const onFinished = vi.fn(async () => {});

    renderHook(() => useResearchJobs(server, { onFinished, intervalMs: 20 }));

    await waitFor(() => expect(onFinished).toHaveBeenCalledTimes(1));
    // Several poll cycles with the same (still unacknowledged) job.
    await act(async () => { await vi.advanceTimersByTimeAsync(100); });
    expect(onFinished).toHaveBeenCalledTimes(1);
    expect(onFinished.mock.calls[0][0].id).toBe('rj_done');
  });

  it('retries delivery when it fails, and never stops polling over it', async () => {
    const server = fakeServer({ running: [], awaiting: [], finished: [{ id: 'rj_done' }] });
    const onFinished = vi.fn()
      .mockRejectedValueOnce(new Error('thread unreadable'))
      .mockResolvedValue(undefined);

    renderHook(() => useResearchJobs(server, { onFinished, intervalMs: 20 }));

    await waitFor(() => expect(onFinished).toHaveBeenCalledTimes(2));
  });

  it('answers a parked run with the person\'s choice, then re-reads the queue', async () => {
    const server = fakeServer({ running: [], awaiting: [{ id: 'rj_ask' }], finished: [] });
    const { result } = renderHook(() => useResearchJobs(server));

    await waitFor(() => expect(result.current.awaiting).toHaveLength(1));
    await act(async () => { await result.current.answer('rj_ask', 'once'); });

    expect(server.calls).toContainEqual(buildResearchApprovalRequest('rj_ask', 'once'));
    expect(server.calls.filter((c) => c.path === 'research/jobs').length).toBeGreaterThan(1);
  });

  it('acknowledging a job drops it from the finished list', async () => {
    const server = fakeServer({ running: [], awaiting: [], finished: [{ id: 'rj_done' }] });
    const { result } = renderHook(() => useResearchJobs(server, { onFinished: async () => {} }));

    await waitFor(() => expect(result.current.finished).toHaveLength(1));
    await act(async () => { await result.current.ack('rj_done'); });

    expect(server.calls).toContainEqual(buildResearchAckRequest('rj_done'));
    expect(result.current.finished).toHaveLength(0);
  });

  it('a failed poll keeps the last known state instead of blanking it', async () => {
    const server = fakeServer({ running: [{ id: 'rj_run' }], awaiting: [], finished: [] });
    const { result } = renderHook(() => useResearchJobs(server, { intervalMs: 20 }));

    await waitFor(() => expect(result.current.running).toHaveLength(1));
    server.request.mockRejectedValue(new Error('server offline'));
    await act(async () => { await vi.advanceTimersByTimeAsync(60); });

    expect(result.current.running).toHaveLength(1);
  });
});
