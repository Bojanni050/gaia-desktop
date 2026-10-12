# Background Voice — Implementation Spec

Status: **revised after Phase 0** (2026-10-12). Audit 2026-10-11; spike results in `background-voice-phase0-findings.md`. No product code written yet.
Scope: persistent voice conversation for Gaia Desktop on Windows 11.

Everything marked **[verified]** was read in this repo or in `Gaia-Cloud` on the
date above. Everything marked **[unverified]** is an assumption to check in
Phase 0 before relying on it.

## 1. Goals

- Continuous voice conversation that keeps listening while the window is minimized or another app has focus.
- Gaia speaks her replies; the user can interrupt her mid-sentence, **on speakers**.
- Optional local wake word "Hey Gaia", toggled independently of continuous mode.
- A master microphone-off switch that disables all capture and wake detection.

## 2. Constraints (from the architecture, kept absolute)

1. Desktop = presence + local capabilities. Cloud = identity, reasoning, memory. Desktop adds no reasoning, personality, memory retrieval, or second conversation engine.
2. Voice turns go through the existing `useConversation.send()` → `server_stream_turn` → `conversation/turn` path, so history, threads and Cloud-side saving behave exactly as for typed turns.
3. Raw microphone audio is never persisted by default.
4. Ambient audio is never sent to a cloud provider for wake-word detection. Wake detection is fully local.
5. Capture, device management, lifecycle, VAD and wake word live in the Rust capability layer.

## 3. Current state

| Area | State | Evidence |
|---|---|---|
| Mic capture | Scaffold only. `AudioInputEngine` trait has no implementor; `input_devices()` returns empty; permission is an in-memory enum starting `Unknown`; `audio_set_permission` only sets the field. | `src-tauri/src/audio/mod.rs` |
| Device selection | `AudioSettings.preferred_input` is persisted but read by nothing. | `src-tauri/src/settings/mod.rs` |
| Capture subsystem | Registry exists, no sources registered. | `src-tauri/src/capture/mod.rs` |
| TTS | Implemented. `speech_synthesize` → Cloud `POST /speech`; bytes played in the webview via `Audio` + Blob URL; gain/mute; 150 ms fade-out; `GET /speech/info` language gate. | `src-tauri/src/speech/mod.rs`, `desktop/src/lib/speech.js`, `desktop/src/lib/language.js` |
| Streaming turn | Implemented, text only, no cancel path. | `src-tauri/src/communication/http.rs:198` |
| Barge-in | Only "new turn stops old speech". `send()` returns early while `busy`. | `desktop/src/state/useConversation.js` |
| Presence orb | `speaking` animation exists but `App.jsx` hard-codes `presenceState = 'quiet'`. | `desktop/src/App.jsx`, `desktop/src/presence/PresenceOrb.jsx` |
| Windows lifecycle | No tray, no close handler, no autostart, no single-instance. Closing the window exits the app. | grep of `src-tauri/` |
| Cloud STT | **Does not exist.** Only `/speech` and `/speech/info` (TTS: Mistral Voxtral, Xiaomi MiMo). | `Gaia-Cloud/services/gaia-api/src/server.js:609,618` |
| Cloud turn abort | No client-disconnect / `AbortController` handling found in `server.js`. [unverified for the rest of the service] | grep of `server.js` |

Other facts: `tokio` features are only `sync, time, rt, fs`; `reqwest` has no websocket feature; the CSP is `connect-src 'self' ipc:` and `media-src 'self' blob:`; CI runs `cargo test --lib` and vitest on `windows-latest`, which has no audio hardware.

## 4. Decisions (confirmed by the product owner)

| Decision | Answer | Consequence |
|---|---|---|
| Cloud STT exists? | Unknown → audit found it does not | Cloud must add `POST /speech/transcribe` |
| Commercial distribution | No | Non-commercial wake-word models are acceptable |
| Output device | Speakers | Acoustic echo cancellation (AEC) is mandatory, not optional |
| Playback location | May move into Rust | Rust owns playback so the playback signal is the AEC reference |

### 4.1 Phase 0 conclusions (2026-10-12)

Settled (see `background-voice-phase0-findings.md` for evidence):

- **Capture:** `cpal` 0.18.2 (Apache-2.0) on WASAPI works on the owner's machine. Devices have **stable ids**, and the stream error callback reports `DeviceChanged`. A stream can open with no error and deliver **zero frames**, so the engine needs a no-frames watchdog. Large buffers in the audio callback cause overruns: the callback must only forward small chunks.
- **AEC:** pure-Rust AEC3 ports build with no C/C++ toolchain; `aec-rs` (needs libclang) and `webrtc-audio-processing` (C++ build) are rejected. **Real-room results on the owner's speakers, five runs** (first-run numbers were not saved; four runs replayed offline, plus the saved run 2): echo removal ranges from 17 dB (far from the mic, cold start) to 58 dB, and was 24 to 27 dB in a repeat of the baseline setup, so it varies a lot between runs. False barge-in events: **0** in 35 s of echo-only audio across the five runs. The user's speech is attenuated by the suppressor while talking over Gaia, so the frame-level speech hit rate is only 34 to 79%, but the product-relevant measure is **onset detection**: 3 consecutive 20 ms frames above threshold appeared **0.8 to 1.6 s after the NOW prompt in all five runs** (this includes the user's reaction time, which the test cannot separate). The `aec3` crate (MIT OR BSD-3-Clause) exposes suppressor tuning (`sonora` does not); relaxing suppression 4x raises the hit rate by about 10 to 16 points for 5 to 10 dB less echo removal. **Decision: use `aec3` with relaxed suppressor tuning**, keep `sonora` as the alternative, and treat voice barge-in on speakers as feasible but conditional (see below). The AEC output gates to near silence while only Gaia speaks, so the VAD needs an absolute noise floor (-46 dBFS used in the spike) and a minimum run length (3 frames). Far from the mic the filter needs time to converge (17 dB cold, 30 to 35 dB after a second pass), so the first reply of a session is the weakest. **Speech-to-text on echo-cancelled audio works** (Mistral `voxtral-mini-latest`, language auto-detected as Dutch): when the user recited a sentence over Gaia (three runs), every echo-cancelled variant transcribed it about as well as the raw mic, with a few word errors in both. Two runs (run 2 and baseline) returned empty or nonsense text for every variant including the raw mic, so they are inconclusive (see findings).
- **Cloud abort (C2):** confirmed missing, but smaller than expected: Hermes and generator streams already accept a `signal`; the route and `performStreamingTurn` just never create or pass one.
- **Wake word, decision: Rustpotter DTW references (Apache-2.0), not the trained-model mode.** Measured on the owner's voice (23 usable "hey" clips, 20 near-miss negatives, 10 minutes of ambient audio):

| Method | Setting | "Hey" hits | Near-miss negatives firing | Ambient false accepts / hour |
|---|---|---|---|---|
| DTW references (5 clips) | threshold 0.50 | 36 of 36 | 14 of 20 | 0 |
| DTW references | threshold 0.57 | 31 of 36 | 6 of 20 | 0 |
| DTW references | threshold 0.60 | 20 of 36 | 0 of 20 | 0 |
| Trained model (tiny) | 0.90 | 3 of 8 | 0 of 4 | 80 |
| Trained model (tiny) | 0.98 | 1 of 8 | 0 of 4 | 0 |

  The trained model has no usable threshold at this data size. DTW references catch every wake word at 0.50 with no ambient false accepts, but fire on deliberate near-misses ("Gaia", "hey Maya"). Caveats: one 10-minute ambient recording of unknown speech content; one recording session; small held-out sets. **Speech-heavy ambient test (done 2026-10-12, 10 minutes of TV, peak level 0.82):** 0 false accepts per hour at thresholds 0.54 and above, about 6 per hour at 0.50 (one event in 10 minutes). One session, language of the audio unrecorded, so treat it as "under roughly 18 per hour", not zero. DTW stays the wake-word choice; openWakeWord is built only if later testing in other rooms or conditions fails.

Consequences carried into this spec:

- The wake word is **speaker-dependent and enrolled by the user** (Phase 4 enrollment flow). `.rpw` reference files store MFCC features, not audio; raw enrollment clips are deleted after features are computed.
- Default threshold about 0.55 (user-adjustable via `wakeSensitivity`), plus a confirmation tone so false accepts are cheap.
- Rustpotter pulls a stale `candle-core 0.2.2` that only compiles with `half` pinned to 2.4.1: commit the pin in `Cargo.lock` and treat Rustpotter as a maintenance risk (fallback: openWakeWord via `ort`).

Still open after Phase 0: why echo removal varies between runs of the same setup (53 dB vs 27 dB), Dutch STT quality with Mistral Voxtral transcription on the echo-cancelled audio, and Cloud's behaviour when a streaming turn is aborted.

## 5. Architecture

```
mic ── cpal/WASAPI ── resample 16 kHz mono ── RAM ring buffer (~2 s, never on disk)
                                  │
                          AEC (reference = Rust playback signal)
                                  │
                   ┌──────────────┴───────────────┐
              VAD + segmenter              wake-word engine (local)
                   │                              │ "Hey Gaia" → arm session
                   └────────── utterance ◄────────┘
                                  │  only VAD-gated speech leaves the device
                                  ▼
                 POST /speech/transcribe (Cloud)  ── transcript
                                  ▼
             Tauri event voice://transcript { final: true, text }
                                  ▼
          useVoiceSession ─► useConversation.send(text)   (existing path)
                                  ▼  server://turn-delta (existing)
                    reply ─► speech_synthesize ─► Rust playback (rodio/cpal out)
                                  │ playback samples ─► AEC reference
```

### 5.1 Ownership

| Layer | Owns |
|---|---|
| Rust `voice` module | Devices, capture, resample, AEC, VAD, segmentation, wake word, STT upload, playback, mic lifecycle, master switch |
| React `useVoiceSession` | Merges Rust mic state with turn state; calls `send()` with final transcripts; drives orb/presence; UI toggles |
| Gaia Cloud | `/speech/transcribe`, provider adapter, turn cancellation, optional voice-modality hint |

### 5.2 Rust voice state machine

`Off → Idle (wake-only) | Armed (continuous) → Capturing → Transcribing → Waiting (Gaia thinking/speaking) → Armed | Idle`

- **Off**: no capture stream exists, no wake engine, buffers zeroed.
- **Idle**: capture + wake engine running, nothing leaves the device.
- **Armed**: a conversation is open; VAD-gated utterances are sent to STT.
- During **Waiting** capture and AEC keep running so the user can interrupt, but utterance upload only starts after a barge-in is confirmed (§5.5).

### 5.3 Commands and events

Commands (new): `voice_get_status`, `voice_list_devices`, `voice_select_device`, `voice_set_mic_enabled`, `voice_set_continuous`, `voice_set_wake_word`, `voice_arm` / `voice_disarm`, `speech_play(text)`, `speech_stop(fade_ms)`, `server_cancel_turn(request_id)`.

Events (new): `voice://state`, `voice://transcript` (`{ id, final, text }`), `voice://level` (throttled, for the UI meter), `voice://error` (`{ code, message }`), `speech://started`, `speech://ended`.

### 5.4 Settings

New `voice` struct in `Settings`, `#[serde(default)]` so legacy `settings.json` still parses (same pattern as the existing `legacy_settings_without_volume_still_parse_at_full_volume` test):

```
voice: {
  micEnabled: false,        // master switch. RUNTIME ONLY (never persisted; serde skip): off at every launch
  continuousEnabled: false,
  wakeWordEnabled: false,
  inputDevice: null,        // cpal device id (e.g. "wasapi:{...}"); null = system default; unknown id falls back to default
  wakeEnrolled: false,      // true once a Rustpotter reference file exists in the config dir
  wakeSensitivity: 0.55,    // maps to the detector threshold; 0.50 catches everything, 0.60 misses ~45% in testing
  bargeInEnabled: true
}
```

Naming rule: `audio.muted` / `audio.volume` remain **playback-only**. Never reuse them for the microphone. `audio.preferredInput` is migrated into `voice.inputDevice` and removed.

### 5.5 Echo prevention and interruption (speakers)

1. Rust playback writes samples to the output device and, in the same callback, to a delay-aligned reference buffer for the AEC.
2. The AEC (`aec3` with relaxed suppression, chosen in Phase 0; more real-room runs pending) subtracts the reference from the capture before VAD and wake word.
3. **Barge-in** fires when VAD detects speech on the *post-AEC* signal for ≥ 250 ms while Gaia is speaking **or** the wake word fires. On barge-in: `speech_stop(150)` (fade, as today), `server_cancel_turn` if a turn is still streaming, then the speech is captured as the next utterance.
4. Safety net if AEC underperforms on a given room: the user setting `bargeInEnabled=false` falls back to half-duplex (capture muted for upload while speaking; interrupt by wake word or hotkey only), and a cooldown of 400 ms follows playback end.

### 5.6 Master mic-off semantics

Off means the cpal `Stream` is **dropped**, not paused: Windows releases the device and the system mic indicator disappears. The wake engine and VAD stop, buffers are zeroed, `voice://state = off` is emitted, and the setting is persisted. No code path may re-enable capture except an explicit user action; update, crash-restart and login always start `Off`.

### 5.7 Turn cancellation

`server_cancel_turn(request_id)` keeps an `AbortHandle` per active `stream_turn` task (a `tokio-util` `CancellationToken` or a `oneshot`). Cancelling drops the `reqwest` response, which closes the connection. What Cloud does with the dropped connection is **[unverified]** (§8, C2).

`useConversation` changes: `cancel()` aborts the active turn; voice-originated sends may supersede a running turn instead of being rejected by the `busy` guard.

### 5.8 Windows lifecycle

- Tauri `tray-icon` feature; closing the window hides to tray while voice is on, otherwise exits as today. Tray menu: show, mic on/off, quit.
- Single-instance plugin so a second launch cannot open a second capture stream.
- Pause capture on session lock and system suspend; resume only if the user had it on and chooses to (default: stay off after resume until unlocked and confirmed by a tray notification).
- Windows' "let desktop apps access your microphone" is an OS setting, not a Tauri capability. Detect denial through capture errors and silence, surface it on the existing settings microphone line, and do not retry in a loop.

## 6. Privacy and security

- No audio is written to disk. Ring buffer and utterance buffers are RAM only and zeroed on stop.
- Wake detection is local. Nothing leaves the device while in Idle.
- In Armed state only VAD-gated utterances are uploaded, over the existing authenticated `ServerLink` (bearer token from the credential store). In continuous mode without a wake word, any speech in the room is uploaded — the UI must show a persistent listening indicator and an always-reachable off switch.
- Logs and error strings must never include audio or transcript text.
- Transcripts become ordinary user turns and are saved by Cloud like typed turns; the desktop stores nothing extra.
- Tauri capability `default.json` gains only what the tray needs; no webview `getUserMedia` is used, so the CSP stays as is.

## 7. Dependencies (versions and licenses to verify in Phase 0)

| Purpose | Candidate | Notes |
|---|---|---|
| Capture + output | `cpal` 0.18 (Apache-2.0) | WASAPI. Persist the stable `DeviceId` string; fall back to default if gone. Stream errors include `DeviceChanged`. Add a no-frames watchdog and keep the audio callback allocation-light. |
| Playback | `rodio` (on `cpal`) | Decode WAV/MP3 from `/speech`. |
| Resample | `rubato` | |
| AEC | `aec3` 0.4 (MIT OR BSD-3-Clause, pure Rust AEC3 port) with relaxed suppressor tuning | Chosen in Phase 0 because it exposes `EchoCanceller3Config`; `sonora` 0.2 (BSD-3-Clause) is the alternative but has no suppression tuning API. Both are young crates (aec3's README calls its API a work in progress). Rejected: `aec-rs` (needs libclang), `webrtc-audio-processing` (C++ toolchain). |
| VAD | Silero via `ort`, or a lightweight VAD crate | |
| Wake word | **Rustpotter** 3.0.2 (Apache-2.0), DTW reference mode | Needs `half` pinned to 2.4.1 (stale `candle-core`). Trained-model mode rejected (see 4.1). openWakeWord fallback only if the speech-heavy ambient test fails. Porcupine rejected (AccessKey validated at init; custom keywords non-commercial; free tier reportedly ended 2026-06-30). |
| Cancellation | `tokio-util` | |
| Tray | tauri `tray-icon` feature | |
| Tokio | add `macros`, `rt-multi-thread` if needed | Currently `sync, time, rt, fs`. |

## 8. Gaia Cloud changes (separate repo)

- **C1. `POST /speech/transcribe`** — authenticated like `/speech`; body is one utterance (16 kHz mono PCM or WAV) plus an optional language hint; returns `{ text, language }`. Provider adapter in `src/speech/` mirroring `mistralTts.js` / `mimoTts.js`; Mistral Voxtral transcription is the first candidate **[unverified]**. Admin routes/config alongside the TTS ones. Must keep the existing "no cognition in speech modules" test style.
- **C2. Turn abort on client disconnect** — confirmed missing in Phase 0. Plumbing: create an `AbortController` in the `conversation/turn` route, abort it on `res.on('close')` before the stream finishes, pass `signal` through `performStreamingTurn` and the `turn.js` Hermes wrapper (it currently forwards only `onDelta`) into `hermes.stream` / `gaiaGenerator.stream`, which already accept `{ signal }`. Save what was streamed and mark it interrupted; do not reflect an interrupted turn. See the Cloud spec for tests.
- **C3. (optional) `modality: "voice"`** on the turn body so Cloud can choose to answer more briefly. Cloud decides; the desktop only reports the modality. Confirm Cloud tolerates unknown body fields first.

## 9. Files likely to change

Rust: `src-tauri/Cargo.toml`, `src/audio/mod.rs` (engine, fake for tests), new `src/voice/{mod,engine,aec,vad,wake,stt,session}.rs`, new `src/speech/playback.rs` (and `speech/mod.rs`), `src/settings/mod.rs`, `src/lib.rs`, `src/communication/{mod,http,client}.rs` (cancel, STT transport), `src/presence/mod.rs`, `capabilities/default.json`, `tauri.conf.json`.

React: new `state/useVoiceSession.js` (+test), `server/api.js` (+test), `state/useConversation.js` (+test), `lib/speech.js` (thin wrapper over Rust commands; keep gain/mute tests), `settings/SettingsPanel.jsx`, `presence/*`, `App.jsx`, `conversation/Composer.jsx`, `lib/lexicon.js`.

Cloud: `services/gaia-api/src/server.js`, new `src/speech/*Stt.js`, `adminRoutes.js`, tests.

## 10. Phases and acceptance tests

### Phase 0 — Spikes and decisions (throwaway code, no product merge)

**Status (2026-10-12): mostly done.** Done: capture, AEC options (synthetic and real room, three runs plus offline replay), Cloud abort audit, wake-word comparison (DTW vs trained), speech-heavy ambient test. Remaining: more real-room echo runs under varied conditions, Dutch STT test (including on echo-cancelled audio). Spike code: branch `spike/voice-phase0`, `spikes/voice-phase0/`.
- Rustpotter vs openWakeWord on the owner's voice, Dutch and English "Hey Gaia".
- AEC spike: Rust playback → reference → AEC, on the owner's actual speakers.
- Mistral transcription API: Dutch accuracy, latency, terms.
- Cloud: what happens to a streaming turn when the client disconnects.
- WebView2 behaviour while hidden is moot once playback is in Rust, but confirm the coordinator survives hide-to-tray.
- **Accept:** written findings; chosen wake engine, AEC crate, VAD and STT provider; measured false-accept/false-reject on a recorded set.

### Phase 1 — Rust audio core and master switch

**Status (2026-10-12): implemented on branch `feat/voice-phase1`** (not yet merged). Built: `audio/capture.rs` (cpal backend, `CaptureBackend` trait), `audio/mod.rs` (`MicController`: master switch, device fallback, no-frames and device-failure watchdogs, level events), `voice` settings with migration of `audio.preferredInput`, `speech/playback.rs` (rodio playback thread with fade-out and stale-synthesis cancellation), `speech_play/stop/set_gain` commands, frontend wrapper and Settings controls. Verified: 39 Rust tests plus 3 opt-in hardware tests, 127 frontend tests, a real-microphone check that Windows' own consent record shows the microphone in use while on and released after off. Not yet done: a full manual run of the packaged app.
Implement the engine behind `AudioInputEngine`, device list/select, `voice` settings, mic on/off, Rust playback replacing `playSpeech` (keeping gain/mute/fade).
- **Accept:**
  - With the switch off, the Windows mic indicator is off and no capture stream exists.
  - Unplugging the selected device falls back to default and emits `voice://error`.
  - A legacy `settings.json` parses; mic starts off.
  - Existing `speech.test.js` and `useConversation.test.js` pass (adapted).
  - `cargo test --lib` passes in CI using the fake engine.

### Phase 2 — Push-to-talk and Cloud STT (C1)

**Status (2026-10-12): implemented on branch `feat/voice-phase2`** (stacked on `feat/voice-phase1`; needs Gaia-Cloud PR #21 deployed with a speech-to-text provider configured). Built: capture tap on the mic sink (a recording sees mono chunks only while one is in progress), `audio/utterance.rs` (speech detection, trim, 16 kHz resample, WAV), `audio/ptt.rs` (`PttController` state machine, events `voice://ptt` and `voice://transcript`, 30 s limit, discard on mic-off or device change), `speech/stt.rs` (`POST /speech/transcribe`, status-to-code mapping), `voice_ptt_*` commands, `usePushToTalk` and a mic button in the composer. Deviation from the original plan: **no live VAD yet**; push-to-talk has the user choose start and stop, and the energy-based speech check runs once over the finished recording (it trims silence and refuses to upload recordings with no speech). Streaming VAD for hands-free mode arrives in Phase 3. Verified: 69 Rust tests (plus 4 opt-in hardware tests; the real microphone delivered exactly 2.00 s at 48 kHz through the tap), the real HTTP call against a local server, 143 frontend tests. Not yet verified: a full spoken question through a Gaia Cloud that has a working speech-to-text provider.
VAD, segmentation, `/speech/transcribe`, `voice://transcript`, mic button in the composer feeding `send()`.
- **Accept:**
  - A spoken question appears in the same thread and History as a typed one.
  - Packet capture shows only VAD-gated utterances leave the device.
  - No audio file is written anywhere.
  - Dutch and English utterances transcribe correctly.

### Phase 3 — Continuous conversation, AEC, interruption (C2)
`useVoiceSession`, tray and hide-on-close, AEC enabled, `server_cancel_turn`, barge-in.
- **Accept:**
  - Continuous mode works while minimized and while another app has focus.
  - With speakers on, Gaia does not respond to her own voice during a 10-minute conversation.
  - Interrupting stops audio within ~200 ms and the interrupted speech becomes the next turn.
  - Cancelling mid-stream leaves a consistent History entry.
  - Master off during speaking silences capture immediately and the tray reflects it.

### Phase 4 — Wake word
Rustpotter DTW references with a user **enrollment flow** (record 5 to 8 clips of "Hey Gaia", features computed, raw clips discarded), independent toggle, sensitivity slider, confirmation tone.
- **Accept:**
  - Enrollment rejects unusable clips (empty, near-silent, over-long, noisy) and asks for a re-record; it refuses to finish with fewer than 5 good clips.
  - On the owner's recorded set: at least 95% of held-out "Hey Gaia" detected at the default sensitivity, and no ambient false accepts in one hour of speech-heavy audio. Phase 0 reference point: at 0.54 DTW detected 35 of 36 with no false accepts in 10 minutes of TV; a one-hour test is still needed.
  - "Hey Gaia" arms a conversation from Idle.
  - Over one hour of TV/music the false-accept count meets the target set in Phase 0.
  - All four combinations of continuous × wake behave as specified; master off disables both.
  - Nothing is uploaded while Idle (packet capture).

### Phase 5 — Quality
Sentence-level TTS streaming for latency, Dutch tuning, level meter polish, optional local STT fallback.

## 11. Failure modes

| Failure | Handling |
|---|---|
| Device unplugged / default changed | Stream error → fall back to default, `voice://error`, notify once |
| OS mic permission denied | Stay off, show state on the settings microphone line, no retry loop |
| Server offline / unauthorized | Stop uploading, keep wake local, single notification; status already tracked by `ServerLink` |
| STT call fails or times out | Discard utterance, calm message, stay armed |
| STT stream drops mid-utterance | Never send a partial sentence as a turn |
| TTS fails | Text reply stands (existing behaviour); no retry loop |
| Echo loop on speakers | AEC + post-playback cooldown + `bargeInEnabled=false` fallback |
| Wake false accepts | Sensitivity setting, short confirmation tone, easy disarm. Near-miss words ("Gaia", "hey Maya") are the known weak spot |
| Stream opens but delivers no frames | Watchdog (5 s for the first frames, 2 s for a later stall) → `voice://error`, stop, do not report "listening". **Security software can cause this:** the owner's Norton blocks a program it does not recognise (including every freshly built dev binary) until it is allowed, and the stream then opens without error but delivers nothing. The error text names this as a likely cause |
| Audio glitch (xrun) in capture | Count and surface; drop the affected utterance rather than send gapped audio |
| Wake word not enrolled | Wake toggle disabled until enrollment completes |
| Crash with mic live | Capture dies with the process; always restarts `Off` |
| Sleep/lock | Pause capture; resume requires user confirmation |

## 12. Open items

- Remaining Phase 0 tests: explain the run-to-run variation in echo removal, Dutch STT with Mistral Voxtral transcription on raw vs echo-cancelled audio.
- Voice barge-in on speakers is feasible but conditional: ship it behind `bargeInEnabled`, keep the wake word / hotkey interruption as the dependable path, and run a longer echo-only soak (an hour of Gaia speaking, user silent) to measure false barge-ins before making it the default. Wake word: re-test in other rooms and at other times before Phase 4 targets are fixed.
- VAD crate choice (Silero via `ort` vs a lightweight crate) — not yet spiked.
- Cloud interruption semantics (C2) and whether Cloud tolerates `modality` (C3).
- Whether the continuous-mode "any speech is uploaded" posture is acceptable or a local-STT option is wanted.
