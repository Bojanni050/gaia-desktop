/**
 * Enrolling "Hey Gaia": the user says it a few times and Gaia learns what it sounds like in their voice.
 *
 * The prompts deliberately vary the way it is said (normal, softer, louder, faster, slower, from a little
 * further away): enrolment calibrates the wake word against that spread, so it keeps recognising them on a
 * tired day or from the other side of the desk, not only when they say it exactly like the first three times.
 * The recordings stay in memory and are thrown away when this closes; what is saved is a features file, not audio.
 */
import React, { useCallback, useEffect, useState } from 'react';
import { voiceApi } from '../server/api';
import { useVoiceStatus } from '../state/useVoiceStatus';
import { L } from '../lib/lexicon';

/** One prompt per recording; the first three are the plain phrase. */
export const PROMPT_KEYS = [
  'wakePromptNormal',
  'wakePromptNormal',
  'wakePromptNormal',
  'wakePromptSofter',
  'wakePromptLouder',
  'wakePromptFaster',
  'wakePromptSlower',
  'wakePromptFurther',
];

const PROBLEM_KEYS = {
  empty: 'wakeProblemEmpty',
  tooQuiet: 'wakeProblemQuiet',
  noSpeech: 'wakeProblemNoSpeech',
  tooShort: 'wakeProblemShort',
  tooLong: 'wakeProblemLong',
  noisy: 'wakeProblemNoisy',
};

const ERROR_KEYS = {
  mic_off: 'wakeErrMicOff',
  mic_interrupted: 'wakeErrInterrupted',
  busy: 'wakeErrBusy',
  enrol_failed: 'wakeErrEnrol',
};

export default function WakeEnrollment({ onClose, onDone, api = voiceApi }) {
  const voice = useVoiceStatus(api);
  const [phase, setPhase] = useState('idle'); // idle | recording | saving
  const [accepted, setAccepted] = useState(0);
  const [min, setMin] = useState(5);
  const [max, setMax] = useState(8);
  const [feedback, setFeedback] = useState(null); // { ok, text }
  const [error, setError] = useState(null);
  const micOn = Boolean(voice.status && voice.status.micEnabled);

  // Whatever happens, leaving throws the recordings away.
  useEffect(() => () => { api.wakeEnrollCancel().catch(() => {}); }, [api]);

  const record = useCallback(async () => {
    setError(null);
    setFeedback(null);
    setPhase('recording');
    try {
      const report = await api.wakeEnrollRecord();
      setAccepted(report.accepted);
      setMin(report.min);
      setMax(report.max);
      setFeedback(
        report.check.ok
          ? { ok: true, text: L.wakeClipGood }
          : { ok: false, text: L[PROBLEM_KEYS[report.check.problem] || 'wakeProblemNoSpeech'] }
      );
    } catch (e) {
      setError(L[ERROR_KEYS[e && e.code]] || L.wakeErrGeneric);
    } finally {
      setPhase('idle');
    }
  }, [api]);

  const finish = useCallback(async () => {
    setError(null);
    setPhase('saving');
    try {
      const status = await api.wakeEnrollFinish();
      if (onDone) onDone(status);
      if (onClose) onClose();
    } catch (e) {
      setError(L[ERROR_KEYS[e && e.code]] || L.wakeErrGeneric);
      setPhase('idle');
    }
  }, [api, onDone, onClose]);

  const prompt = L[PROMPT_KEYS[Math.min(accepted, PROMPT_KEYS.length - 1)]];
  const done = accepted >= max;

  return (
    <div className="wake-enrol" role="dialog" aria-label={L.wakeEnrolTitle}>
      <h4>{L.wakeEnrolTitle}</h4>
      <p className="capability-line">{L.wakeEnrolIntro}</p>

      {!micOn ? (
        <>
          <p className="capability-line mic-note">{L.wakeNeedMic}</p>
          <button type="button" onClick={() => voice.setEnabled(true)}>
            {L.micSwitchOn}
          </button>
        </>
      ) : (
        <>
          <div className="wake-dots" aria-label={`${accepted} / ${max}`}>
            {Array.from({ length: max }, (_, i) => (
              <span key={i} className={`wake-dot${i < accepted ? ' is-done' : ''}`} />
            ))}
          </div>
          {!done && <p className="wake-prompt">{prompt}</p>}
          {phase === 'recording' && <p className="capability-line mic-note">{L.wakeRecording}</p>}
          {feedback && phase !== 'recording' && (
            <p className={`capability-line ${feedback.ok ? 'wake-ok' : 'mic-note'}`} role="status">
              {feedback.text}
            </p>
          )}
          {error && (
            <p className="capability-line mic-note" role="alert">
              {error}
            </p>
          )}
          <div className="field-row">
            {!done && (
              <button type="button" onClick={record} disabled={phase !== 'idle'}>
                {accepted === 0 ? L.wakeRecord : L.wakeRecordNext}
              </button>
            )}
            <button type="button" onClick={finish} disabled={phase !== 'idle' || accepted < min}>
              {phase === 'saving' ? L.wakeSaving : L.wakeFinish}
            </button>
            <button type="button" onClick={onClose} disabled={phase === 'saving'}>
              {L.wakeCancel}
            </button>
          </div>
          <p className="capability-line">{L.wakeEnrolPrivacy}</p>
        </>
      )}
    </div>
  );
}
