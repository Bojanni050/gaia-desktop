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
  },
  en: {
    newPage: 'Begin a page',
    untitled: 'Untitled page',
    composerPlaceholder: 'Say anything, or just think out loud…',
    healthWhisper: "I can't reach my reason engine right now. Take your time — I'm here when you're ready to try again.",
    retry: 'Retry',
    footLine1: 'A lifelong personal intelligence,',
    footLine2: 'growing through understanding.',
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
