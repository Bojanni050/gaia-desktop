import { describe, it, expect, vi } from 'vitest';
import { renderHook, act, waitFor } from '@testing-library/react';
import { useKairosLiveEpisodes } from './useKairosLiveEpisodes';

/** A fake server whose `onEpisode` we can fire by hand. */
function fakeServer() {
  let handler = null;
  return {
    onEpisode(fn) {
      handler = fn;
      return Promise.resolve(() => { handler = null; });
    },
    emit(episode) {
      if (handler) handler(episode);
    },
  };
}

describe('useKairosLiveEpisodes', () => {
  it('collects live episodes newest-first and de-duplicates by id', async () => {
    const server = fakeServer();
    const { result } = renderHook(() => useKairosLiveEpisodes(server));

    act(() => server.emit({ id: 'e1', summary: 'one' }));
    act(() => server.emit({ id: 'e2', summary: 'two' }));
    act(() => server.emit({ id: 'e1', summary: 'one again' })); // duplicate

    await waitFor(() => expect(result.current.episodes).toHaveLength(2));
    expect(result.current.episodes.map((e) => e.id)).toEqual(['e2', 'e1']);
  });

  it('reports connected once an episode arrives', async () => {
    const server = fakeServer();
    const { result } = renderHook(() => useKairosLiveEpisodes(server));

    expect(result.current.status).toBe('connecting');
    act(() => server.emit({ id: 'e1' }));
    await waitFor(() => expect(result.current.status).toBe('connected'));
  });

  it('ignores episodes without an id', async () => {
    const server = fakeServer();
    const { result } = renderHook(() => useKairosLiveEpisodes(server));

    act(() => server.emit({ summary: 'no id' }));
    act(() => server.emit(null));
    expect(result.current.episodes).toHaveLength(0);
  });

  it('unsubscribes on unmount', async () => {
    const unsubscribe = vi.fn();
    const server = { onEpisode: vi.fn(async () => unsubscribe) };
    const { unmount } = renderHook(() => useKairosLiveEpisodes(server));

    await waitFor(() => expect(server.onEpisode).toHaveBeenCalled());
    unmount();
    expect(unsubscribe).toHaveBeenCalled();
  });
});
