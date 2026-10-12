/**
 * Two switches, always within reach: the microphone (the master switch: off means nothing is captured and
 * nothing is listening for "Hey Gaia" either) and continuous conversation (Gaia keeps listening and answers
 * when you speak).
 *
 * They are only buttons: Rust owns the microphone and the listening state, and this reads them back from
 * events. Switching continuous conversation on also switches the microphone on, which is what was just
 * asked for; switching the microphone off ends listening altogether.
 */
import React, { useState } from 'react';
import { AudioLines, Mic, MicOff } from 'lucide-react';
import { voiceApi } from '../server/api';
import { useVoiceStatus } from '../state/useVoiceStatus';
import { L } from '../lib/lexicon';

/** Listening states in which Gaia is following the conversation (waiting for "Hey Gaia" is not one). */
const CONVERSING = ['listening', 'hearing', 'transcribing'];

export default function VoiceControls({ listenState = 'off', api = voiceApi }) {
  const voice = useVoiceStatus(api);
  const [busy, setBusy] = useState(false);
  const micOn = Boolean(voice.status && voice.status.micEnabled);
  const continuousOn = micOn && CONVERSING.includes(listenState);

  const toggleMic = async () => {
    setBusy(true);
    try {
      await voice.setEnabled(!micOn);
    } finally {
      setBusy(false);
    }
  };

  const toggleContinuous = async () => {
    setBusy(true);
    try {
      await api.continuousSet(!continuousOn);
      if (!continuousOn && !micOn) await voice.setEnabled(true);
    } catch (_) {
      /* the shell is unreachable: the button simply does not take effect */
    } finally {
      setBusy(false);
    }
  };

  const micLabel = micOn ? L.voiceMicSwitchOff : L.voiceMicSwitchOn;
  const continuousLabel = continuousOn ? L.voiceContinuousSwitchOff : L.voiceContinuousSwitchOn;

  return (
    <div className="voice-controls" role="group" aria-label={L.voiceControlsLabel}>
      <button
        type="button"
        className={`voice-control${micOn ? ' is-on' : ''}`}
        onClick={toggleMic}
        disabled={busy}
        aria-pressed={micOn}
        aria-label={micLabel}
        title={micLabel}
      >
        {micOn ? <Mic size={16} /> : <MicOff size={16} />}
      </button>
      <button
        type="button"
        className={`voice-control${continuousOn ? ' is-on' : ''}${listenState === 'hearing' ? ' is-hearing' : ''}`}
        onClick={toggleContinuous}
        disabled={busy}
        aria-pressed={continuousOn}
        aria-label={continuousLabel}
        title={continuousLabel}
      >
        <AudioLines size={16} />
      </button>
    </div>
  );
}
