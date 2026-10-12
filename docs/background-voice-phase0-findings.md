# Background Voice — Phase 0 Findings (in progress)

Branch `spike/voice-phase0`, throwaway code in `spikes/voice-phase0/` (own Cargo workspace, not part of the Tauri build).
Run on the owner's Windows 11 machine, 2026-10-11. Companion to `background-voice-spec.md`.

## Done

### Capture (cpal 0.18.2, Apache-2.0) — works
- WASAPI enumeration lists two inputs (Steinberg UR22 line, HyperX SoloCast mic) and four outputs; defaults detected.
- **cpal 0.18 has stable device ids** (`DeviceId`, persisted via `Display`/`FromStr`, e.g. `wasapi:{0.0.1.00000000}.{41429e7a-…}`). This supersedes the spec's "persist the name" advice: persist the id, fall back to default if it is gone.
- The stream error callback has `ErrorKind::DeviceChanged` — device change handling does not need polling.
- Capture at the SoloCast default: 48 kHz, 2 ch, f32, 480-frame (10 ms) buffers. Dropping the stream took ~42 ms.
- Windows registers the exe under `ConsentStore\microphone\NonPackaged\<path>`. The dev build and the installed build are different paths, so each gets its own OS consent entry.
- **Silent failure mode:** the same binary launched from the Claude Bash tool opened the stream without error but delivered **zero frames**; from PowerShell it delivered 48 000 frames/s. A denied or blocked mic can therefore look like "running, no data". The engine needs a no-frames watchdog (e.g. no callbacks for 2 s → `voice://error`), not just error callbacks.

### Echo cancellation — pure-Rust options build; synthetic results promising
- `aec-rs` needs libclang (bindgen); not installed here and not in CI → reject. `webrtc-audio-processing` wraps the C++ library (needs meson/clang toolchain) → reject.
- **`sonora` 0.2.0 (BSD-3-Clause, pure Rust port of WebRTC AEC3 + NS + AGC2)** and `aec3` 0.4.0 (MIT OR BSD-3-Clause) build with no extra toolchain. Both are young crates; sonora's API is the simpler.
- Synthetic test (`cargo run --release -- aec-sim`): speech-like far-end leaking through a delayed multi-tap "room" into the mic, 48 kHz mono.
  - Far-end only: ERLE 19 dB (0–2 s) → 35 dB (2–5 s) → ~59 dB (5–10 s), for 30 ms and 120 ms delays.
  - Double talk (user speaks over Gaia, 12–16 s): output energy is preserved (0.12 vs 0.135 rms) but the output lags by ~12 ms and the best lagged correlation with the clean near-end is only ~0.5. Possible causes: near-end partly suppressed, or residual echo. The synthetic near-end shares the far-end's harmonic structure, which is a hard case, so this is pessimistic, but **double-talk quality is unproven**.
- **Not tested:** the real room. A real speakers-plus-mic run is still required (see "Needs you").

### Cloud turn abort — confirmed missing (C2 is needed)
- `performStreamingTurn` (`Gaia-Cloud/services/gaia-api/src/turn.js:1002`) receives `res` but no abort signal, and `server.js` registers no `req`/`res` close listener around it.
- `hermesClient.stream` and `gaiaGenerator.stream` **already accept `{ signal }`**, and `turn.js:209` wraps Hermes' `stream` but forwards only `onDelta`. So C2 is mostly plumbing: create an `AbortController` in the route, abort on `res.on('close')` before finish, thread `signal` through `performStreamingTurn` → generator/hermes. Smaller than expected.

### Wake-word crates
- `rustpotter` 3.0.2 (Apache-2.0) builds **only after pinning `half` to 2.4.1** (`cargo update -p half --precise 2.4.1`): its dependency `candle-core 0.2.2` is stale and fails against current `half`. A pin in `Cargo.lock` works but signals maintenance risk. No wake-word accuracy measured yet.

### Cloud STT provider (web, secondhand)
- Mistral lists Voxtral Mini Transcribe 2 on `/v1/audio/transcriptions` at ~$0.003/min and a Realtime variant at ~$0.006/min. Dutch is listed for the original Voxtral; the Transcribe 2 language list was not confirmed. Not tested against the real API.

## Needs the owner (cannot be done without your voice, speakers or key)

1. **Wake word:** record ~10 "Hey Gaia" (Dutch pronunciation) + ~10 other phrases; then compare Rustpotter vs openWakeWord false accept/reject. I can build the record/score harness first.
2. **Real-room AEC:** play Gaia's TTS through the speakers while you speak; compare sonora vs aec3 on the real mic. Needs the harness to play audio (rodio) and record, nothing saved unless you ask.
3. **Dutch STT:** a Mistral API key (or the one already in Cloud's provider store) to test `/v1/audio/transcriptions` with Dutch audio.

## Spec updates to make after Phase 0
- Device ids replace names (cpal 0.18).
- AEC candidate: sonora (pure Rust) — no C/C++ toolchain, `aec-rs` / `webrtc-audio-processing` rejected.
- C2 scope reduced to abort-signal plumbing; add the Cloud-side file/line references above.
- Add the no-frames watchdog to the engine requirements and failure modes.
- Add the half-pin / candle risk to the Rustpotter notes.

## Wake-word harness (built, validated on synthetic audio only)

Run from `spikes/voice-phase0/` in **PowerShell** (not from tools that sandbox audio, see the zero-frames finding above):

```powershell
cargo build --release
.\target\release\voice-phase0.exe collect hey 12 2.0     # 12 x "Hey Gaia", 2 s each
.\target\release\voice-phase0.exe collect neg 20 2.0     # 20 other phrases / near-misses ("Gaia", "hey Maya", sentences)
.\target\release\voice-phase0.exe collect bg 1 600       # 10 min ambient: TV, music, talking (false-accept test)
.\target\release\voice-phase0.exe score 5                # build ref from 5 "hey" clips, test the rest
```

- Clips are 16 kHz mono WAV in `spikes/voice-phase0/recordings/<label>/` — **gitignored; it is your voice, do not commit or upload it.** Delete the folder when done.
- Optional last argument of `collect` is a device id from `devices` (default = system default mic).
- `score` runs two folds (refs = first N / last N `hey` clips), sweeps five thresholds, and prints held-out hits/misses, false accepts on the `neg` clips and false accepts per hour on the `bg` audio. `GAIA_TRACE=1` prints per-clip best scores; `GAIA_SELFTEST=1` scores the reference clips themselves (sanity check).
- Validation: on synthetic tone clips the harness hits 14/14 held-out at thresholds 0.40–0.50 with 0 negative/ambient false accepts, and false-accepts at 0.30. This proves the pipeline, **not** Rustpotter's real accuracy — that needs your recordings.
- Lessons built in: clips are padded with 1 s leading and 1.5 s trailing silence, because Rustpotter only emits a detection while its window slides across the word and after a countdown; without padding even an identical clip never fired.
- openWakeWord is **not** in the harness yet (it is Python/ONNX; a Rust runner via `ort` is a separate step). Compare it only if Rustpotter's numbers are weak.

### Trained-model mode (harness)

```powershell
.\target\release\voice-phase0.exe train 1500 tiny     # epochs, model size (tiny|small|medium|large)
.\target\release\voice-phase0.exe score-model         # held-out 30% only
```

- Split is deterministic (every 3rd `hey`/`neg` clip held out; ambient audio is cut in time, first 70% trains the `none` class, last 30% is the false-accept test). Model is saved to `recordings/_model.rpw` (gitignored).
- Validated on synthetic tone data only (pipeline runs end to end; synthetic numbers are meaningless). Real accuracy needs your recordings.
- Reading the result: the real question is whether some threshold gives high held-out `hey` hits **and** low `neg` / `bg` false accepts, which the DTW reference mode did not on near-miss negatives.

### Speech-heavy ambient test (2026-10-12)

10 minutes of TV (peak level 0.82), scored against the five-reference DTW wake word with `GAIA_BG_LABEL=bgs ... score 5`:

| Threshold | "Hey" hits (of 36) | Near-miss negatives firing (of 20) | Ambient false accepts / hour |
|---|---|---|---|
| 0.50 | 36 | 14 | 6 |
| 0.54 | 35 | 11 | 0 |
| 0.57 | 31 | 6 | 0 |
| 0.60 | 20 | 0 | 0 |

One session; the audio's language was not recorded. Zero events in 10 minutes supports only "under roughly 18 per hour". The near-miss weakness is unchanged.
