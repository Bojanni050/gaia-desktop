/**
 * Push-to-talk: the mic button's behaviour.
 *
 *   microphone off  -> a click switches the microphone on (an explicit act; nothing records yet)
 *   idle            -> a click starts a recording
 *   recording       -> a click ends it (Esc cancels); the text arrives as `onTranscript`
 *
 * The hook only mirrors what Rust reports (`voice://ptt`, `voice://transcript`) and forwards the
 * transcript to its caller, which sends it as an ordinary turn. It never interprets the words: that
 * stays with Gaia Cloud. Audio never reaches this layer at all; recording, trimming and the upload
 * happen in Rust.
 */
import { useCallback, useEffect, useRef, useState } from 'react';
import { voiceApi, serverApi, getSttInfo } from '../server/api';
import { useVoiceStatus } from './useVoiceStatus';

/** How long "no speech heard" / a failure stays on the button before it settles back to idle. */
export const NOTICE_MS = 4000;

export function usePushToTalk({ onTranscript, api = voiceApi, request = serverApi.request } = {}) {
  const voice = useVoiceStatus(api);
  const [phase, setPhase] = useState('idle'); // idle | recording | transcribing | noSpeech | failed
  const [error, setError] = useState(null);
  const [available, setAvailable] = useState(true);
  const handler = useRef(onTranscript);
  handler.current = onTranscript;

  useEffect(() => {
    let active = true;
    const unlisten = [];
    const keep = (promise) =>
      Promise.resolve(promise)
        .then((fn) => {
          if (!active) fn && fn();
          else if (fn) unlisten.push(fn);
        })
        .catch(() => {});

    keep(
      api.onPtt((s) => {
        if (!active || !s) return;
        setPhase(s.state);
        setError(s.state === 'failed' ? { code: s.code, message: s.message } : null);
      })
    );
    keep(
      api.onTranscript((t) => {
        // Hands-free transcripts belong to useVoiceSession; this button acts only on its own.
        if (active && t && t.source !== 'continuous' && typeof t.text === 'string' && handler.current) {
          handler.current(t.text, t);
        }
      })
    );
    // Hide the button when Cloud says it cannot transcribe. If it cannot say (offline, an older
    // server), assume yes: a real failure then explains itself when it happens.
    getSttInfo(request).then((info) => {
      if (active && info && info.configured === false) setAvailable(false);
    });
    return () => {
      active = false;
      unlisten.forEach((fn) => fn());
    };
  }, [api, request]);

  // "No speech" and failures are messages, not states to stay in.
  useEffect(() => {
    if (phase !== 'noSpeech' && phase !== 'failed') return undefined;
    const timer = setTimeout(() => setPhase('idle'), NOTICE_MS);
    return () => clearTimeout(timer);
  }, [phase]);

  const cancel = useCallback(() => {
    api.pttCancel().catch(() => {});
  }, [api]);

  // Esc abandons a recording in progress; nothing is sent.
  useEffect(() => {
    if (phase !== 'recording') return undefined;
    const onKey = (e) => {
      if (e.key === 'Escape') cancel();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [phase, cancel]);

  const micOn = Boolean(voice.status && voice.status.micEnabled);

  const toggle = useCallback(async () => {
    if (!micOn) {
      await voice.setEnabled(true);
      return;
    }
    if (phase === 'recording') {
      api.pttStop().catch(() => {});
      return;
    }
    if (phase === 'transcribing') return;
    try {
      setError(null);
      await api.pttStart();
    } catch (e) {
      setError(e && e.code ? { code: e.code, message: e.message } : { code: 'unknown', message: String(e) });
      setPhase('failed');
    }
  }, [api, micOn, phase, voice]);

  return { phase, error, available, micOn, micError: voice.status && voice.status.error, toggle, cancel };
}
