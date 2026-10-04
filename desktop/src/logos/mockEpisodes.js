/**
 * Mock episodes for the Logos timeline.
 *
 * These are demonstrations, not fixtures: the shape matches what Logos is
 * expected to hand the UI (see the Episode type in the design brief), but the
 * content is invented so the card can be judged on real-looking material.
 * `screenshot` is a tiny inline SVG thumb so the raw-evidence rows show what a
 * real capture thumbnail will look like without shipping binary assets.
 */

/** A stand-in capture thumbnail: window chrome + a couple of text lines. */
function shot(label, tone = '#2A241B') {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="176" height="120" viewBox="0 0 176 120">
    <rect width="176" height="120" fill="#1C1811"/>
    <rect width="176" height="16" fill="${tone}"/>
    <circle cx="9" cy="8" r="2.5" fill="#C98B7A"/>
    <circle cx="17" cy="8" r="2.5" fill="#CBA36A"/>
    <circle cx="25" cy="8" r="2.5" fill="#A9C79A"/>
    <text x="10" y="42" fill="#A79E8E" font-family="monospace" font-size="9">${label}</text>
    <rect x="10" y="54" width="122" height="6" rx="3" fill="#2A241B"/>
    <rect x="10" y="66" width="98" height="6" rx="3" fill="#2A241B"/>
    <rect x="10" y="78" width="142" height="6" rx="3" fill="#2A241B"/>
  </svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
}

export const EPISODES = [
  {
    id: 'ep-q3-budget',
    startTime: '2026-10-03T10:15:00',
    endTime: '2026-10-03T10:42:00',
    session: 'Budgetherziening Q3',
    title: 'Budgetherziening Q3',
    interpretation:
      'Bojan bekeek een e-mail van Bas over de budgetherziening voor Q3 en werkte vervolgens de financiële tabel bij in Excel.',
    confidence: 0.86,
    context: [
      { application: 'Outlook', object: 'RE: Q3 Budget', type: 'email' },
      { application: 'Excel', object: 'Financieel_Model_v3.xlsx', type: 'document' },
    ],
    observations: [
      {
        id: 'obs-q3-1',
        timestamp: '2026-10-03T10:15:12',
        application: 'Outlook',
        windowTitle: 'RE: Q3 Budget — Bojan',
        ocrText:
          'Van: Bas van der Meer\nAan: Bojan\nOnderwerp: RE: Q3 Budget\n\nHoi Bojan,\n\nZou je voor de budgetherziening van Q3 de cijfers van vorige maand nog willen aanvullen in het model? Dan kunnen we donderdag doorpakken.\n\nGroet,\nBas',
        screenshot: shot('Outlook'),
        uia: { controlType: 'Document', name: 'RE: Q3 Budget', automationId: 'mail-body' },
      },
      {
        id: 'obs-q3-2',
        timestamp: '2026-10-03T10:21:41',
        application: 'Outlook',
        windowTitle: 'RE: Q3 Budget — Concept',
        ocrText: 'Concept\n\nHoi Bas,\n\nIk vul de cijfers vandaag aan en stuur je vanmiddag een update.',
        screenshot: shot('Concept'),
        uia: { controlType: 'Edit', name: 'Berichttekst', value: 'Hoi Bas, …' },
      },
      {
        id: 'obs-q3-3',
        timestamp: '2026-10-03T10:29:03',
        application: 'Excel',
        windowTitle: 'Financieel_Model_v3.xlsx — Excel',
        ocrText:
          'Budget Q3\tPrognose\tWerkelijk\tVerschil\nMarketing\t120.000\t118.400\t-1.600\nR&D\t340.000\t351.200\t+11.200\nOperations\t210.000\t205.900\t-4.100',
        screenshot: shot('Excel', '#2A241B'),
        uia: { controlType: 'Table', name: 'Blad1', rowCount: 42, columnCount: 8 },
      },
      {
        id: 'obs-q3-4',
        timestamp: '2026-10-03T10:38:55',
        application: 'Excel',
        windowTitle: 'Financieel_Model_v3.xlsx — Opgeslagen',
        ocrText: 'Cellen B14:E16 bijgewerkt. Bestand opgeslagen.',
        screenshot: shot('Opgeslagen'),
        uia: { controlType: 'Window', name: 'Financieel_Model_v3.xlsx', saved: true },
      },
    ],
  },
  {
    id: 'ep-ochtendmail',
    startTime: '2026-10-03T09:04:00',
    endTime: '2026-10-03T09:19:00',
    session: null,
    title: 'Ochtendmail en planning',
    interpretation:
      "Bojan liep 's ochtends de inbox door, bekeek de planning voor de week en opende de sprintpagina in de browser.",
    confidence: 0.71,
    context: [
      { application: 'Outlook', object: 'Inbox', type: 'email' },
      { application: 'Teams', object: 'Weekplanning', type: 'application' },
      { application: 'Chrome', object: 'Sprint 42 — Jira', type: 'webpage' },
    ],
    observations: [
      {
        id: 'obs-ochtend-1',
        timestamp: '2026-10-03T09:04:40',
        application: 'Outlook',
        windowTitle: 'Inbox — Bojan',
        ocrText: '12 ongelezen. Afzenders: Bas van der Meer, HR, Sprint-notificaties.',
        screenshot: shot('Inbox'),
        uia: { controlType: 'List', name: 'Berichtenlijst', itemCount: 12 },
      },
      {
        id: 'obs-ochtend-2',
        timestamp: '2026-10-03T09:11:08',
        application: 'Teams',
        windowTitle: 'Weekplanning — Teams',
        ocrText: 'Maandag 10:00 Budgetherziening · Donderdag 14:00 Sprint review',
        screenshot: shot('Teams'),
        uia: { controlType: 'Calendar', name: 'Weekplanning' },
      },
      {
        id: 'obs-ochtend-3',
        timestamp: '2026-10-03T09:17:52',
        application: 'Chrome',
        windowTitle: 'Sprint 42 - Jira - Google Chrome',
        ocrText: 'Sprint 42 · 7 open · 3 in uitvoering · 2 te reviewen',
        screenshot: shot('Jira'),
        uia: { controlType: 'Document', name: 'Sprint board', automationId: 'ghx-pool' },
      },
    ],
  },
  {
    id: 'ep-api-debug',
    startTime: '2026-10-03T11:02:00',
    endTime: '2026-10-03T11:35:00',
    session: 'API-integratie',
    title: 'Fout opsporen in sync-endpoint',
    interpretation:
      'Bojan zocht een fout op in het sync-endpoint: hij las de documentatie, vergeleek de request in de browser en liep de logs na in de terminal.',
    confidence: 0.79,
    context: [
      { application: 'Chrome', object: 'Sync API — docs', type: 'webpage' },
      { application: 'VS Code', object: 'syncClient.ts', type: 'file' },
      { application: 'Windows Terminal', object: 'gaia-api — npm run dev', type: 'application' },
    ],
    observations: [
      {
        id: 'obs-api-1',
        timestamp: '2026-10-03T11:02:23',
        application: 'Chrome',
        windowTitle: 'Sync API — docs',
        ocrText: 'POST /sync\nVerwacht: { items: Item[], cursor?: string }\nAntwoord 200: { accepted: number }',
        screenshot: shot('Docs'),
        uia: { controlType: 'Document', name: 'Sync API reference' },
      },
      {
        id: 'obs-api-2',
        timestamp: '2026-10-03T11:09:47',
        application: 'Chrome',
        windowTitle: 'DevTools — Network',
        ocrText: '400 Bad Request · syncClient.ts:88 · "cursor" moet een string zijn',
        screenshot: shot('DevTools'),
        uia: { controlType: 'Table', name: 'Network requests', itemCount: 6 },
      },
      {
        id: 'obs-api-3',
        timestamp: '2026-10-03T11:16:31',
        application: 'VS Code',
        windowTitle: 'syncClient.ts — gaia-api',
        ocrText: 'cursor: number | undefined → verwacht string',
        screenshot: shot('VS Code'),
        uia: { controlType: 'Edit', name: 'Editor', line: 88 },
      },
      {
        id: 'obs-api-4',
        timestamp: '2026-10-03T11:24:12',
        application: 'Windows Terminal',
        windowTitle: 'gaia-api — npm run dev',
        ocrText: 'POST /sync 400 · ongeldige cursor\ncontroleer parser van het request-lichaam',
        screenshot: shot('Terminal'),
        uia: { controlType: 'Document', name: 'Terminal output' },
      },
      {
        id: 'obs-api-5',
        timestamp: '2026-10-03T11:33:05',
        application: 'VS Code',
        windowTitle: 'syncClient.ts — gaia-api',
        ocrText: "cursor: String(cursor ?? '') — test groen",
        screenshot: shot('Groen', '#3A4A38'),
        uia: { controlType: 'Edit', name: 'Editor', line: 88 },
      },
    ],
  },
];
