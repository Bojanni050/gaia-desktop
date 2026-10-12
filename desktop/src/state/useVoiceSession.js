/**
 * Hands-free conversation: the coordinator between what Rust hears and the conversation.
 *
 * Rust keeps the microphone open, finds speech as it happens, has Gaia Cloud transcribe each utterance
 * and reports through events. This hook turns those events into conversation:
 *
 *   transcript (source "continuous") -> an ordinary turn, replacing a turn still running
 *   barge-in (the user talked over Gaia) -> stop her voice, cancel the turn still streaming
 *
 * It owns no audio and no interpretation: Rust owns the microphone and Gaia Cloud owns what the words
 * mean. Push-to-talk transcripts (source "ptt") are the composer's, never this hook's.
 */
import { useEffect, useRef, useState } from 'react';
import { voiceApi } from '../server/api';
import { stopSpeech } from '../lib/speech';

export function useVoiceSession({ conversation, api = voiceApi }) {
  // off | waiting (for "Hey Gaia") | listening | hearing | transcribing | failed
  const [state, setState] = useState('off');
  const [error, setError] = useState(null);
  // Always the latest conversation (its callbacks change with every turn) without resubscribing.
  const convo = useRef(conversation);
  convo.current = conversation;

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

    Promise.resolve(api.continuousState())
      .then((on) => active && on && setState((s) => (s === 'off' ? 'listening' : s)))
      .catch(() => {});

    keep(
      api.onContinuous((s) => {
        if (!active || !s) return;
        setState(s.state);
        setError(s.state === 'failed' ? { code: s.code, message: s.message } : null);
      })
    );
    keep(
      api.onTranscript((t) => {
        if (!active || !t || t.source !== 'continuous') return;
        const text = typeof t.text === 'string' ? t.text.trim() : '';
        if (!text) return;
        convo.current.send(text, [], { interrupt: true });
      })
    );
    keep(
      api.onBargeIn(() => {
        if (!active) return;
        // Rust already faded her voice out; this makes sure nothing is queued behind it and the reply
        // that is still streaming in stops, so Cloud can stop generating.
        stopSpeech();
        convo.current.cancel();
      })
    );
    return () => {
      active = false;
      unlisten.forEach((fn) => fn());
    };
  }, [api]);

  return { state, error, active: state !== 'off' };
}
