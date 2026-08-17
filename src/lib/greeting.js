/**
 * Greeting — part of the ongoing relationship, not a static label.
 * Ported from Gaia Web so the desktop opens the same way.
 */
const NAME = 'Bo';

function pick(arr) {
  return arr[Math.floor(Math.random() * arr.length)];
}

export function getGreeting() {
  const currentLang = localStorage.getItem('gaia.lang') || 'nl';
  const firstArrival = !localStorage.getItem('gaia.arrived');

  if (firstArrival) {
    localStorage.setItem('gaia.arrived', new Date().toISOString());
    return {
      title: currentLang === 'nl' ? 'Hallo. Ik ben Gaia.' : "Hello. I'm Gaia.",
      first: true,
    };
  }

  const h = new Date().getHours();
  let timed;
  if (currentLang === 'nl') {
    timed =
      h < 5
        ? `Nog wakker, ${NAME}?`
        : h < 12
          ? `Goedemorgen, ${NAME}.`
          : h < 18
            ? `Goedemiddag, ${NAME}.`
            : `Goedenavond, ${NAME}.`;
  } else {
    timed =
      h < 5
        ? `Still up, ${NAME}?`
        : h < 12
          ? `Good morning, ${NAME}.`
          : h < 18
            ? `Good afternoon, ${NAME}.`
            : `Good evening, ${NAME}.`;
  }

  const title =
    currentLang === 'nl'
      ? pick([timed, timed, 'Welkom terug.', 'Fijn om je weer te zien.', `Klaar om verder te gaan, ${NAME}?`])
      : pick([timed, timed, 'Welcome back.', 'Nice to see you again.', `Ready to continue, ${NAME}?`]);

  return { title, first: false };
}
