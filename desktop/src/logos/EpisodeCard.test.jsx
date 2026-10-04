import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent, cleanup } from '@testing-library/react';
import EpisodeCard from './EpisodeCard';
import { L } from '../lib/lexicon';

afterEach(cleanup);

function episode(overrides = {}) {
  return {
    id: 'ep-1',
    startTime: '2026-10-03T10:15:00',
    endTime: '2026-10-03T10:42:00',
    session: 'Budgetherziening Q3',
    title: 'Budgetherziening Q3',
    interpretation: 'Bojan bekeek een e-mail van Bas en werkte de tabel bij in Excel.',
    confidence: 0.86,
    context: [
      { application: 'Outlook', object: 'RE: Q3 Budget', type: 'email' },
      { application: 'Excel', object: 'Financieel_Model_v3.xlsx', type: 'document' },
    ],
    observations: [
      {
        id: 'obs-1',
        timestamp: '2026-10-03T10:15:12',
        application: 'Outlook',
        windowTitle: 'RE: Q3 Budget — Bojan',
        ocrText: 'Zou je de cijfers van vorige maand aanvullen?',
        uia: { controlType: 'Document', name: 'RE: Q3 Budget' },
      },
      {
        id: 'obs-2',
        timestamp: '2026-10-03T10:29:03',
        application: 'Excel',
        windowTitle: 'Financieel_Model_v3.xlsx — Excel',
        ocrText: 'Marketing 120.000 118.400 -1.600',
      },
    ],
    ...overrides,
  };
}

function renderCard(ep) {
  const onAskGaia = vi.fn();
  const onSavePattern = vi.fn();
  render(<EpisodeCard episode={ep} onAskGaia={onAskGaia} onSavePattern={onSavePattern} />);
  return { onAskGaia, onSavePattern };
}

describe('EpisodeCard', () => {
  it('shows the interpretation as dominant, with the time range, session and interpretation badge', () => {
    const ep = episode();
    renderCard(ep);

    expect(screen.getByText(ep.interpretation)).toBeTruthy();
    expect(screen.getByText(/10:15\s*–\s*10:42/)).toBeTruthy();
    expect(screen.getByText(new RegExp(`Sessie: ${ep.session}`))).toBeTruthy();
    expect(screen.getByText(L.logosEpisodeBadge)).toBeTruthy();
  });

  it('keeps the raw observations hidden until asked, behind an accessible toggle', () => {
    const ep = episode();
    renderCard(ep);

    const toggle = screen.getByRole('button', { name: new RegExp(L.logosEvidenceToggle) });
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('region', { name: L.logosRawObservations })).toBeNull();
    expect(screen.queryByText('Zou je de cijfers van vorige maand aanvullen?')).toBeNull();

    fireEvent.click(toggle);

    expect(toggle.getAttribute('aria-expanded')).toBe('true');
    expect(screen.getByRole('region', { name: L.logosRawObservations })).toBeTruthy();
    // The toggle points the region it opens at.
    expect(document.getElementById(toggle.getAttribute('aria-controls'))).toBeTruthy();
    // Both captures, with their own app and window title, surfaced verbatim.
    expect(screen.getByText('Zou je de cijfers van vorige maand aanvullen?')).toBeTruthy();
    expect(screen.getByText('Marketing 120.000 118.400 -1.600')).toBeTruthy();
    expect(screen.getByText('RE: Q3 Budget — Bojan')).toBeTruthy();
    expect(screen.getByText('Financieel_Model_v3.xlsx — Excel')).toBeTruthy();
  });

  it('marks the evidence as observation, not Logos text, and names the capture count', () => {
    renderCard(episode());
    const toggle = screen.getByRole('button', { name: new RegExp(L.logosEvidenceToggle) });
    expect(toggle.textContent).toContain('2');
    fireEvent.click(toggle);
    expect(screen.getByText(L.logosRawObservations)).toBeTruthy();
    expect(screen.getByText(L.logosRawNote)).toBeTruthy();
  });

  it('collapses again on a second activation', () => {
    renderCard(episode());
    const toggle = screen.getByRole('button', { name: new RegExp(L.logosEvidenceToggle) });
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(toggle.getAttribute('aria-expanded')).toBe('false');
    expect(screen.queryByRole('region', { name: L.logosRawObservations })).toBeNull();
  });

  it('is keyboard-focusable as a native control', () => {
    renderCard(episode());
    const toggle = screen.getByRole('button', { name: new RegExp(L.logosEvidenceToggle) });
    toggle.focus();
    expect(document.activeElement).toBe(toggle);
  });

  it('renders each context object as an app chip', () => {
    const ep = episode();
    renderCard(ep);
    expect(screen.getByText('Outlook')).toBeTruthy();
    expect(screen.getByText('“RE: Q3 Budget”')).toBeTruthy();
    expect(screen.getByText('Excel')).toBeTruthy();
    expect(screen.getByText('“Financieel_Model_v3.xlsx”')).toBeTruthy();
  });

  it('hands the whole episode to the two actions', () => {
    const ep = episode();
    const { onAskGaia, onSavePattern } = renderCard(ep);

    fireEvent.click(screen.getByRole('button', { name: L.logosAskGaia }));
    expect(onAskGaia).toHaveBeenCalledWith(ep);

    fireEvent.click(screen.getByRole('button', { name: L.logosSavePattern }));
    expect(onSavePattern).toHaveBeenCalledWith(ep);
  });
});
