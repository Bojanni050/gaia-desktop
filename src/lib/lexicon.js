/**
 * Gaia's lexicon — ported from Gaia Web so both clients speak with one
 * voice. Dutch is the default. Only the strings the desktop uses.
 */
export const LANGUAGES = {
  nl: {
    newPage: 'Begin een pagina',
    untitled: 'Naamloze pagina',
    composerPlaceholder: 'Zeg wat je wilt, of denk hardop…',
    healthWhisper:
      'Ik kan mijn denkmotor momenteel niet bereiken. Neem je tijd — ik ben er als je klaar bent om het opnieuw te proberen.',
    retry: 'Opnieuw proberen',
    footLine1: 'Een levenslange persoonlijke intelligentie,',
    footLine2: 'die groeit door begrip.',
    settings: 'Instellingen',
    settingsTitle: 'Instellingen',
    settingsCloud: 'Gaia Cloud',
    settingsServerUrl: 'Server-URL',
    settingsAuthToken: 'Authenticatietoken',
    settingsTest: 'Verbinding testen',
    settingsTesting: 'Testen…',
    settingsBehaviour: 'Gedrag',
    settingsNotifications: 'Notificaties',
    settingsQuiet: 'Stille aanwezigheid (niet storen)',
    settingsCapabilities: 'Lokale mogelijkheden',
    settingsMicrophone: 'Microfoon',
    settingsCaptureSources: 'Capturebronnen',
    settingsCaptureNone: 'nog geen geregistreerd',
    settingsSave: 'Bewaren',
    settingsSaving: 'Behouden…',
    settingsSaved: 'Bewaard',
    settingsClose: 'Sluiten',
    deleteConversation: 'Gesprek verwijderen',
    send: 'Versturen',
    // turn failure phrases (phrases.js reads these)
    turnNoServer: 'Gaia is nog niet verbonden met haar server. Je kunt er een toevoegen in de instellingen.',
    turnUnreachable: 'Gaia is nu even niet te bereiken.',
    turnCapture: 'Dit apparaat kon dat niet vastleggen.',
    turnFallback: 'Er ging hier iets mis. Je bericht staat er nog.',
  },
  en: {
    newPage: 'Begin a page',
    untitled: 'Untitled page',
    composerPlaceholder: 'Say anything, or just think out loud…',
    healthWhisper: "I can't reach my reason engine right now. Take your time — I'm here when you're ready to try again.",
    retry: 'Retry',
    footLine1: 'A lifelong personal intelligence,',
    footLine2: 'growing through understanding.',
    settings: 'Settings',
    settingsTitle: 'Settings',
    settingsCloud: 'Gaia Cloud',
    settingsServerUrl: 'Server URL',
    settingsAuthToken: 'Auth token',
    settingsTest: 'Test connection',
    settingsTesting: 'Testing…',
    settingsBehaviour: 'Behaviour',
    settingsNotifications: 'Notifications',
    settingsQuiet: 'Quiet presence (do not disturb)',
    settingsCapabilities: 'Local capabilities',
    settingsMicrophone: 'Microphone',
    settingsCaptureSources: 'Capture sources',
    settingsCaptureNone: 'none registered yet',
    settingsSave: 'Save',
    settingsSaving: 'Saving…',
    settingsSaved: 'Saved',
    settingsClose: 'Close',
    deleteConversation: 'Delete conversation',
    send: 'Send',
    turnNoServer: 'Gaia is not connected to her server yet. You can add one in settings.',
    turnUnreachable: 'Gaia cannot be reached right now.',
    turnCapture: 'This device could not capture that.',
    turnFallback: 'Something went wrong on this side. Your message is still here.',
  },
};

const defaultLang = 'nl';

export const L = new Proxy(
  {},
  {
    get(target, prop) {
      const currentLang = localStorage.getItem('gaia.lang') || defaultLang;
      const langData = LANGUAGES[currentLang] || LANGUAGES[defaultLang];
      return langData[prop] || LANGUAGES.en[prop] || prop;
    },
  }
);
