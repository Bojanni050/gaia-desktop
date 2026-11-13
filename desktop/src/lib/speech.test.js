/**
 * The playback wrapper is thin on purpose: the audio is fetched, decoded and played in Rust. These
 * tests pin what the wrapper owes the rest of the app: it forwards to the right Tauri commands, keeps
 * the local gain, treats an interrupted reply as a normal end, and reports a real failure.
 */
import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => ({ outcome: 'ended' })) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock('@tauri-apps/plugin-dialog', () => ({ open: vi.fn(), save: vi.fn() }));

import { invoke } from '@tauri-apps/api/core';
import { playSpeech, stopSpeech, setSpeechGain, speechSilenced, speaksReplies, speaksFirstTwoSentences, SPEECH_FADE_MS } from './speech';

beforeEach(() => {
  invoke.mockReset();
  invoke.mockResolvedValue({ outcome: 'ended' });
  setSpeechGain({ volume: 1, muted: false, speakFirstTwoSentences: false });
  invoke.mockClear();
});

describe('playSpeech', () => {
  it('hands the reply text to Rust, which synthesizes and plays it', async () => {
    await expect(playSpeech('hello there')).resolves.toBeUndefined();
    expect(invoke).toHaveBeenCalledWith('speech_play', { text: 'hello there', language: null });
  });
  it('passes the detected language so the matching voice is used', async () => {
    await playSpeech('hallo daar', 'nl');
    expect(invoke).toHaveBeenCalledWith('speech_play', { text: 'hallo daar', language: 'nl' });
  });


  it('treats a reply that was stopped on purpose as a normal end, not a failure', async () => {
    invoke.mockResolvedValue({ outcome: 'stopped' });
    await expect(playSpeech('cut me off')).resolves.toBeUndefined();
  });

  it('rejects when the reply could not be spoken', async () => {
    invoke.mockResolvedValue({ outcome: 'failed', message: 'could not open the speakers' });
    await expect(playSpeech('hi')).rejects.toThrow('could not open the speakers');
  });

  it('rejects when the shell itself errors', async () => {
    invoke.mockRejectedValue(new Error('speech request failed'));
    await expect(playSpeech('hi')).rejects.toThrow('speech request failed');
  });
});

describe('stopSpeech', () => {
  it('asks Rust to ease the reply out over the fade time', () => {
    stopSpeech();
    expect(invoke).toHaveBeenCalledWith('speech_stop', { fadeMs: SPEECH_FADE_MS });
  });

  it('never throws, even when the shell is unreachable', () => {
    invoke.mockRejectedValue(new Error('no shell'));
    expect(() => stopSpeech()).not.toThrow();
  });
});

describe('speaksReplies', () => {
  it('is on until switched off, and switching it off does not touch the volume', () => {
    setSpeechGain({ speakReplies: true });
    expect(speaksReplies()).toBe(true);
    setSpeechGain({ speakReplies: false, volume: 0.4 });
    expect(speaksReplies()).toBe(false);
    expect(speechSilenced()).toBe(false);
    setSpeechGain({ volume: 0.6 }); // a gain change alone leaves the switch as it was
    expect(speaksReplies()).toBe(false);
    setSpeechGain({ speakReplies: true });
    expect(speaksReplies()).toBe(true);
  });
});

describe('speaksFirstTwoSentences', () => {
  it('is off until switched on, and speaks long replies whole', async () => {
    expect(speaksFirstTwoSentences()).toBe(false);
    await playSpeech('One. Two. Three. Four.');
    expect(invoke).toHaveBeenCalledWith('speech_play', { text: 'One. Two. Three. Four.', language: null });
  });

  it('speaks only the first two sentences of a longer reply when switched on', async () => {
    setSpeechGain({ speakFirstTwoSentences: true });
    expect(speaksFirstTwoSentences()).toBe(true);
    await playSpeech('One. Two. Three. Four.', 'en');
    expect(invoke).toHaveBeenCalledWith('speech_play', { text: 'One. Two.', language: 'en' });
  });

  it('leaves a reply of two sentences or fewer untouched when switched on', async () => {
    setSpeechGain({ speakFirstTwoSentences: true });
    await playSpeech('Eerste zin. Tweede zin.', 'nl');
    expect(invoke).toHaveBeenCalledWith('speech_play', { text: 'Eerste zin. Tweede zin.', language: 'nl' });
  });

  it('is a speaking setting only: the volume switch stays where it was', () => {
    setSpeechGain({ speakReplies: true, volume: 0.5 });
    setSpeechGain({ speakFirstTwoSentences: true });
    expect(speaksReplies()).toBe(true);
    expect(speechSilenced()).toBe(false);
  });
});

describe('setSpeechGain', () => {
  it('sends the clamped gain to Rust', () => {
    setSpeechGain({ volume: 5 });
    expect(invoke).toHaveBeenLastCalledWith('speech_set_gain', { volume: 1, muted: false });
    setSpeechGain({ volume: 0.4, muted: true });
    expect(invoke).toHaveBeenLastCalledWith('speech_set_gain', { volume: 0.4, muted: true });
  });

  it('knows when playback would be silent', () => {
    setSpeechGain({ volume: 0.5, muted: true });
    expect(speechSilenced()).toBe(true);
    setSpeechGain({ volume: 0, muted: false });
    expect(speechSilenced()).toBe(true);
    setSpeechGain({ volume: 0.5, muted: false });
    expect(speechSilenced()).toBe(false);
  });

  it('keeps the other field when only one is given', () => {
    setSpeechGain({ volume: 0.3, muted: false });
    setSpeechGain({ muted: true });
    expect(invoke).toHaveBeenLastCalledWith('speech_set_gain', { volume: 0.3, muted: true });
  });

  it('survives an unreachable shell', () => {
    invoke.mockRejectedValue(new Error('no shell'));
    expect(() => setSpeechGain({ volume: 0.2 })).not.toThrow();
  });
});
