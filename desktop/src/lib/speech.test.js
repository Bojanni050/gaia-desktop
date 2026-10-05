/**
 * The playback wrapper's one invariant that matters: only one reply is ever
 * spoken, and a new reply (or a new turn) stops whatever came before it.
 *
 * jsdom has no audio engine and no object-URL support, so both are faked
 * here. The assertions are about which element got played, paused and
 * released, never about sound.
 */
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { playSpeech, stopSpeech, setSpeechGain, speechSilenced } from './speech';

class FakeAudio {
  constructor(src) {
    this.src = src;
    this.volume = 1;
    this.paused = false;
    this.listeners = {};
  }
  addEventListener(type, fn) {
    (this.listeners[type] ||= []).push(fn);
  }
  emit(type) {
    (this.listeners[type] || []).forEach((fn) => fn());
  }
  play() {
    return Promise.resolve();
  }
  pause() {
    this.paused = true;
  }
}

let made;

beforeEach(() => {
  made = [];
  global.Audio = class extends FakeAudio {
    constructor(src) {
      super(src);
      made.push(this);
    }
  };
  URL.createObjectURL = vi.fn(() => 'blob:fake');
  URL.revokeObjectURL = vi.fn();
  setSpeechGain({ volume: 1, muted: false });
  stopSpeech();
});

afterEach(() => {
  stopSpeech();
});

const clip = () => new Uint8Array([1, 2, 3]);

describe('playSpeech barge-in', () => {
  it('stops and releases the previous clip when a new one starts', async () => {
    const first = playSpeech(clip(), 'audio/mpeg');
    const firstAudio = made[0];
    const second = playSpeech(clip(), 'audio/mpeg');
    const secondAudio = made[1];

    expect(firstAudio.paused).toBe(true); // the old reply was cut off
    expect(secondAudio.paused).toBe(false); // the new one is the one speaking
    expect(URL.revokeObjectURL).toHaveBeenCalled();

    // Being stopped on purpose resolves the clip — it is not a failure.
    await expect(first).resolves.toBeUndefined();
    secondAudio.emit('ended');
    await expect(second).resolves.toBeUndefined();
  });

  it('applies the current volume to the element it plays', () => {
    setSpeechGain({ volume: 0.4, muted: false });
    playSpeech(clip());
    expect(made[0].volume).toBe(0.4);
  });

  it('plays muted as silence', () => {
    setSpeechGain({ volume: 1, muted: true });
    expect(speechSilenced()).toBe(true);
    playSpeech(clip());
    expect(made[0].volume).toBe(0);
  });

  it('resolves a clip that is stopped before it ends', async () => {
    const pending = playSpeech(clip());
    stopSpeech();
    await expect(pending).resolves.toBeUndefined();
  });

  it('has nothing left to stop once playback has ended', async () => {
    const pending = playSpeech(clip());
    made[0].emit('ended');
    await pending;
    expect(() => stopSpeech()).not.toThrow();
  });
});

describe('setSpeechGain', () => {
  it('clamps volume to the 0..1 range', () => {
    setSpeechGain({ volume: 5 });
    expect(speechSilenced()).toBe(false);
    setSpeechGain({ volume: -3 });
    expect(speechSilenced()).toBe(true);
  });
});
