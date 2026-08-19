import { useState, useEffect, useRef } from 'react';
import { presenceMessages, defaultPresenceMessage } from './presenceMessages';

/**
 * usePresenceAnimator — ported from Gaia Web.
 * Sequences through the thinking messages at calm intervals (0s, 3s, 7s…).
 */
export function usePresenceAnimator(isActive, type = 'general') {
  const [messageIndex, setMessageIndex] = useState(0);
  const timerRef = useRef(null);

  const messages = presenceMessages[type] || presenceMessages['general'];

  useEffect(() => {
    if (!isActive) {
      setMessageIndex(0);
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      return;
    }

    setMessageIndex(0);

    const scheduleNext = (currentIndex) => {
      if (currentIndex >= messages.length - 1) return;
      const delays = [3000, 4000, 4000, 4000];
      const delay = delays[currentIndex] || 4000;

      timerRef.current = setTimeout(() => {
        setMessageIndex(currentIndex + 1);
        scheduleNext(currentIndex + 1);
      }, delay);
    };

    scheduleNext(0);

    return () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    };
  }, [isActive, messages.length]);

  return messages[messageIndex] || defaultPresenceMessage;
}
