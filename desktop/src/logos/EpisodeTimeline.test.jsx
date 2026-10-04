import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, cleanup } from '@testing-library/react';
import EpisodeTimeline from './EpisodeTimeline';
import {
  buildEpisodeListRequest,
  buildEpisodeEvidenceRequest,
  parseEpisodeList,
  parseEpisodeEvidence,
  adaptKairosEpisode,
} from '../state/contract';
import { L } from '../lib/lexicon';

// The timeline subscribes to live episodes via serverApi (Tauri `listen`),
// which is absent under jsdom. The hooks swallow that (`.catch`), so the
// injected/static tests render without needing a mock; live-merge is covered in
// contract + hook level below.

afterEach(cleanup);

describe('episode seam', () => {
  it('lists episodes over the generic server_request seam (Kairos path)', () => {
    expect(buildEpisodeListRequest()).toEqual({ method: 'get', path: 'kairos/episodes' });
  });

  it('carries page/limit/since as query parameters', () => {
    expect(buildEpisodeListRequest({ page: 2, limit: 10, since: '2026-10-01T00:00:00Z' })).toEqual({
      method: 'get',
      path: 'kairos/episodes?page=2&limit=10&since=2026-10-01T00%3A00%3A00Z',
    });
  });

  it('addresses one episode evidence by id', () => {
    expect(buildEpisodeEvidenceRequest('kei_1')).toEqual({
      method: 'get',
      path: 'kairos/episodes/kei_1/evidence',
    });
  });

  it('adapts a Kairos episode into the card shape', () => {
    const adapted = adaptKairosEpisode({
      id: 'kei_1',
      start_time: '2026-10-03T10:15:00Z',
      end_time: '2026-10-03T10:42:00Z',
      summary: 'Bojan bekeek de inbox.',
      primary_app: 'Outlook',
      involved_apps: ['Outlook', 'Excel'],
      epistemic_status: 'interpretation',
      sources: ['chronicle:ingest:o1'],
    });
    expect(adapted.title).toBe('Outlook');
    expect(adapted.interpretation).toBe('Bojan bekeek de inbox.');
    expect(adapted.epistemicStatus).toBe('interpretation');
    expect(adapted.context).toEqual([
      { application: 'Outlook', type: 'application' },
      { application: 'Excel', type: 'application' },
    ]);
    expect(adapted.observations).toEqual([]);
  });

  it('treats a malformed envelope as "none yet" rather than throwing', () => {
    expect(parseEpisodeList({ body: {} })).toEqual([]);
    expect(parseEpisodeList(undefined)).toEqual([]);
    expect(parseEpisodeList({ body: { data: [{ id: 'a', involved_apps: [] }] } })).toHaveLength(1);
  });

  it('maps the evidence envelope into observation rows', () => {
    const rows = parseEpisodeEvidence({
      body: {
        observations: [
          { object: { id: 'ingest:o1', content: 'OCR tekst', tags: ['Outlook'], title: 'Outlook — Inbox', occurred_at: '2026-10-03T10:15:00Z' } },
        ],
      },
    });
    expect(rows[0].application).toBe('Outlook');
    expect(rows[0].ocrText).toBe('OCR tekst');
    expect(rows[0].windowTitle).toBe('Outlook — Inbox');
    expect(parseEpisodeEvidence({ body: {} })).toEqual([]);
  });
});

describe('EpisodeTimeline', () => {
  it('renders injected episodes without fetching', () => {
    const eps = [
      {
        id: 'ep-1',
        startTime: '2026-10-03T10:15:00',
        endTime: '2026-10-03T10:42:00',
        title: 'T',
        interpretation: 'I',
        context: [],
        observations: [],
      },
    ];
    render(<EpisodeTimeline episodes={eps} onClose={() => {}} />);
    expect(screen.getByText('I')).toBeTruthy();
  });

  it('shows an empty state when the injected list is empty', () => {
    render(<EpisodeTimeline episodes={[]} onClose={() => {}} />);
    expect(screen.getByText(L.logosEmpty)).toBeTruthy();
  });
});
