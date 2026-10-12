//! Echo cancellation: removing Gaia's own voice from what the microphone hears, so she does not answer
//! herself and so the user can interrupt her.
//!
//! The canceller is fed in lock step: for every 10 ms of microphone audio it also gets the 10 ms of
//! what Gaia was *playing* (silence when she is quiet), and returns the microphone audio with her voice
//! taken out. It is AEC3, the WebRTC echo canceller, via the pure-Rust `aec3` crate, with the suppressor
//! relaxed four-fold: the Phase 0 measurements on the owner's speakers showed the default suppression
//! muting the user's own speech while they talked over Gaia, and the relaxed setting trading a few dB of
//! echo removal for noticeably more of the user's voice getting through.

use std::collections::VecDeque;

use aec3::api::config::{DominantNearendDetection, EchoCanceller3Config, MaskingThresholds, Tuning};
use aec3::nodes::audio::AudioFormat;
use aec3::pipelines::linear::{self, LinearPipeline};

/// Samples per 10 ms block at 16 kHz: what the canceller takes and returns at a time.
pub const BLOCK: usize = 160;
/// How the render and capture streams are expected to be offset on arrival. The speaker-to-microphone
/// path measured about 60 ms on the owner's machine; the canceller finds the exact value itself.
const INITIAL_DELAY_MS: i32 = 40;

/// AEC3's suppressor with every threshold relaxed by `k` (and the "user is talking" detector made more
/// sensitive), i.e. less eager to mute the microphone when it hears speech over the echo.
pub fn relaxed_config(k: f32) -> EchoCanceller3Config {
    let t = |a: f32, b: f32, e: f32| MaskingThresholds::new(a, b, e);
    let mut c = EchoCanceller3Config::default();
    c.suppressor.normal_tuning = Tuning::new(t(0.3 * k, 0.4 * k, 0.3), t(0.07 * k, 0.1 * k, 0.3), 2.0, 0.25);
    c.suppressor.nearend_tuning = Tuning::new(t(1.09 * k, 1.1 * k, 0.3), t(0.1 * k, 0.3 * k, 0.3), 2.0, 0.25);
    c.suppressor.dominant_nearend_detection = DominantNearendDetection {
        enr_threshold: 0.25 * k,
        enr_exit_threshold: 10.0,
        snr_threshold: 30.0 / k,
        hold_duration: (50.0 * k) as usize,
        trigger_threshold: 12,
        use_during_initial_phase: true,
        use_unbounded_echo_spectrum: true,
    };
    c
}

pub struct EchoCanceller {
    pipeline: LinearPipeline,
}

impl EchoCanceller {
    pub fn new() -> Result<Self, String> {
        Self::with_config(relaxed_config(4.0))
    }

    pub fn with_config(config: EchoCanceller3Config) -> Result<Self, String> {
        let format = AudioFormat::ten_ms(16_000, 1);
        let pipeline = linear::builder(format, format)
            .initial_delay_ms(INITIAL_DELAY_MS)
            .aec3_config(config)
            // AEC only: the speech recognizer does its own robustness work, and these would colour the audio
            .enable_noise_suppression(false)
            .enable_gain_controller2(false)
            .enable_post_filter(false)
            .build()
            .map_err(|e| format!("could not build the echo canceller: {e:?}"))?;
        Ok(Self { pipeline })
    }

    /// One 10 ms block: `render` is what was played, `capture` what the microphone heard. Writes the
    /// microphone audio with the echo removed to `out`. If the canceller cannot produce output, the
    /// microphone audio passes through untouched (hearing an echo beats hearing nothing).
    pub fn process(&mut self, render: &[f32], capture: &[f32], out: &mut [f32]) {
        debug_assert!(render.len() == BLOCK && capture.len() == BLOCK && out.len() == BLOCK);
        let fed = self.pipeline.handle_render_frame(render).is_ok();
        let produced = fed && self.pipeline.process_capture_frame(capture, out).unwrap_or(false);
        if !produced {
            out.copy_from_slice(capture);
        }
    }
}

/// A level gate for what is left after echo cancellation while Gaia is speaking.
///
/// A linear canceller never removes all of a room's echo (speaker distortion, device clocks that drift
/// apart, driver "enhancements" all leave some), and what is left is loud enough for the speech detector to
/// take for the user. This learns how much is left in *this* room, as a fraction of how loudly she is
/// playing, and quiets everything below a margin above that, so only speech clearly louder than her own
/// leftover echo reaches the detector. The user talking over her is well above it; her echo is not. It
/// adapts: in a room that leaks more the gate rises, so she does not answer herself there either.
pub struct EchoGate {
    ratio: f32,
    recent_ref: VecDeque<f32>,
    ratios: VecDeque<f32>,
}

/// Where the learned fraction starts: about 10 dB of echo removal, what a typical desktop room manages.
const GATE_START_RATIO: f32 = 0.35;
/// How far above her leftover echo speech must be to pass (about 4 dB).
const GATE_MARGIN: f32 = 1.6;
/// Reference quieter than this is silence: nothing is gated and nothing is learned.
const GATE_MIN_REF_RMS: f32 = 0.003;
/// Frames of reference remembered, because her echo reaches the microphone some tens of milliseconds late.
const GATE_REF_FRAMES: usize = 8;
/// Frames of leftover-to-reference ratios the estimate is taken from (about 2 s).
const GATE_WINDOW: usize = 100;

fn rms_of(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

impl EchoGate {
    pub fn new() -> Self {
        Self { ratio: GATE_START_RATIO, recent_ref: VecDeque::new(), ratios: VecDeque::new() }
    }

    /// The learned leftover-echo fraction (for diagnostics and tests).
    pub fn ratio(&self) -> f32 {
        self.ratio
    }

    /// One 20 ms frame of cleaned microphone audio and what was being played over the same 20 ms. Quiets the
    /// frame if it is no louder than her own leftover echo.
    pub fn apply(&mut self, frame: &mut [f32], played: &[f32]) {
        let (frame_rms, ref_rms) = (rms_of(frame), rms_of(played));
        self.recent_ref.push_back(ref_rms);
        if self.recent_ref.len() > GATE_REF_FRAMES {
            self.recent_ref.pop_front();
        }
        let ref_recent = self.recent_ref.iter().copied().fold(0.0f32, f32::max);
        if ref_recent < GATE_MIN_REF_RMS {
            return; // she is not playing: speech is left alone
        }
        if ref_rms >= GATE_MIN_REF_RMS {
            self.ratios.push_back(frame_rms / ref_recent);
            if self.ratios.len() > GATE_WINDOW {
                self.ratios.pop_front();
            }
            if self.ratios.len() >= 25 {
                let mut sorted: Vec<f32> = self.ratios.iter().copied().collect();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let p80 = sorted[(sorted.len() * 4 / 5).min(sorted.len() - 1)];
                self.ratio = 0.9 * self.ratio + 0.1 * p80.clamp(0.05, 2.0);
            }
        }
        if frame_rms < GATE_MARGIN * self.ratio * ref_recent {
            frame.fill(0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = 16_000;

    struct Lcg(u64);
    impl Lcg {
        fn next(&mut self) -> f32 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
        }
    }

    /// Speech-like: harmonics of a wandering pitch under a syllable-rate envelope.
    fn speechlike(seed: u64, secs: usize, f0: f32) -> Vec<f32> {
        let mut r = Lcg(seed);
        let (mut ph, mut env, mut pitch) = (0f32, 0f32, f0);
        (0..SR * secs)
            .map(|i| {
                if i % (SR / 4) == 0 {
                    env = if r.next() > -0.3 { 0.6 + 0.4 * r.next().abs() } else { 0.0 };
                }
                if i % (SR / 50) == 0 {
                    pitch = f0 * (1.0 + 0.15 * r.next());
                }
                ph += std::f32::consts::TAU * pitch / SR as f32;
                let s: f32 = (1..=8).map(|h| (ph * h as f32).sin() / h as f32).sum();
                0.25 * env * s + 0.005 * r.next()
            })
            .collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn db(a: f32, b: f32) -> f32 {
        20.0 * (a.max(1e-9) / b.max(1e-9)).log10()
    }

    /// The room: Gaia's voice reaches the microphone 60 ms late, through a few decaying reflections.
    fn room(render: &[f32]) -> Vec<f32> {
        let taps: [(usize, f32); 4] = [(0, 1.0), (SR / 200, 0.5), (SR / 90, 0.3), (SR / 40, 0.15)];
        let delay = 60 * SR / 1000;
        (0..render.len())
            .map(|i| taps.iter().filter(|(t, _)| i >= delay + t).map(|(t, g)| 0.5 * g * render[i - delay - t]).sum())
            .collect()
    }

    fn run(render: &[f32], mic: &[f32]) -> Vec<f32> {
        run_with(EchoCanceller::new().expect("canceller"), render, mic)
    }

    fn run_with(mut aec: EchoCanceller, render: &[f32], mic: &[f32]) -> Vec<f32> {
        let mut out = vec![0.0f32; mic.len()];
        for b in 0..mic.len() / BLOCK {
            let r = &render[b * BLOCK..(b + 1) * BLOCK];
            let m = &mic[b * BLOCK..(b + 1) * BLOCK];
            aec.process(r, m, &mut out[b * BLOCK..(b + 1) * BLOCK]);
        }
        out
    }

    fn seg(x: &[f32], a: f32, b: f32) -> &[f32] {
        &x[(a * SR as f32) as usize..(b * SR as f32) as usize]
    }

    #[test]
    fn it_removes_gaias_own_voice_from_the_microphone() {
        let render = speechlike(1, 10, 130.0);
        let mic = room(&render);
        let out = run(&render, &mic);
        let erle = db(rms(seg(&mic, 5.0, 9.0)), rms(seg(&out, 5.0, 9.0)));
        assert!(erle >= 20.0, "echo removed: {erle:.1} dB (target at least 20)");
    }

    #[test]
    fn it_lets_the_user_through_while_gaia_is_talking() {
        let render = speechlike(1, 12, 130.0);
        let user = speechlike(2, 12, 210.0);
        let echo = room(&render);
        // the user starts talking at 8 s
        let mic: Vec<f32> = (0..render.len()).map(|i| echo[i] + if i >= 8 * SR { user[i] } else { 0.0 }).collect();
        let out = run(&render, &mic);
        let before = rms(seg(&out, 5.0, 8.0));
        let during = rms(seg(&out, 9.0, 12.0));
        assert!(during > 8.0 * before.max(1e-5), "residual {before:.5}, with the user talking {during:.5}");
    }

    #[test]
    fn when_gaia_is_silent_the_user_is_left_alone() {
        let silence = vec![0.0f32; SR * 6];
        let user = speechlike(3, 6, 180.0);
        let out = run(&silence, &user);
        let (i, o) = (rms(seg(&user, 2.0, 6.0)), rms(seg(&out, 2.0, 6.0)));
        assert!(o > 0.7 * i, "input {i:.4} -> output {o:.4}: speech must pass through when nothing is playing");
    }

    fn frame_of(level: f32) -> Vec<f32> {
        (0..320).map(|i| level * (i as f32 * 0.3).sin()).collect()
    }

    #[test]
    fn the_gate_quiets_leftover_echo_but_lets_louder_speech_through() {
        let mut gate = EchoGate::new();
        let played = frame_of(0.2);
        // what is left of her voice: about a third of what she plays
        for _ in 0..60 {
            let mut left = frame_of(0.07);
            gate.apply(&mut left, &played);
            assert!(left.iter().all(|v| *v == 0.0), "leftover echo is quieted");
        }
        let mut user = frame_of(0.2); // clearly louder than her echo
        gate.apply(&mut user, &played);
        assert!(user.iter().any(|v| *v != 0.0), "the user talking over her passes");
    }

    #[test]
    fn the_gate_learns_a_room_that_leaks_more() {
        let mut gate = EchoGate::new();
        let played = frame_of(0.2);
        let start = gate.ratio();
        for _ in 0..150 {
            let mut left = frame_of(0.12); // 60% of what she plays comes back
            gate.apply(&mut left, &played);
        }
        assert!(gate.ratio() > start * 1.3, "ratio {:.2} -> {:.2}", start, gate.ratio());
        let mut left = frame_of(0.12);
        gate.apply(&mut left, &played);
        assert!(left.iter().all(|v| *v == 0.0), "in this room that level is her echo");
    }

    #[test]
    fn the_gate_leaves_speech_alone_when_she_is_not_playing() {
        let mut gate = EchoGate::new();
        let mut user = frame_of(0.01);
        gate.apply(&mut user, &[0.0; 320]);
        assert!(user.iter().any(|v| *v != 0.0));
    }

    #[test]
    fn a_fresh_canceller_never_panics_on_odd_input() {
        let mut aec = EchoCanceller::new().unwrap();
        let mut out = vec![0.0f32; BLOCK];
        aec.process(&[0.0; BLOCK], &[1.0; BLOCK], &mut out);
        aec.process(&[1.0; BLOCK], &[-1.0; BLOCK], &mut out);
        assert!(out.iter().all(|v| v.is_finite()));
    }
    /// Exploratory: how well the end of a reply is cancelled when the speakers add delay (an external
    /// speaker or a monitor over HDMI adds 100 to 400 ms on top of the room).
    #[test]
    #[ignore = "exploratory measurement; prints, asserts nothing"]
    fn probe_the_end_of_a_reply_with_a_slow_output_device() {
        for delay_ms in [60usize, 150, 250, 400] {
            let reply = speechlike(1, 6, 130.0);
            let mut render = reply.clone();
            render.extend(vec![0.0f32; SR * 3]); // the reference ends with the reply
            let taps: [(usize, f32); 4] = [(0, 1.0), (SR / 200, 0.5), (SR / 90, 0.3), (SR / 40, 0.15)];
            let delay = delay_ms * SR / 1000;
            let mic: Vec<f32> = (0..render.len())
                .map(|i| taps.iter().filter(|(t, _)| i >= delay + t).map(|(t, g)| 0.5 * g * render[i - delay - t]).sum())
                .collect();
            let out = run(&render, &mic);
            let w = |a: f32, b: f32| (rms(seg(&mic, a, b)), rms(seg(&out, a, b)));
            // windows: steady speech, the last second of the reply (reference), the tail after the reference ends
            let (m1, o1) = w(3.0, 5.0);
            let (m2, o2) = w(5.0, 6.0);
            let (m3, o3) = w(6.0, 6.0 + delay_ms as f32 / 1000.0 + 0.05);
            let (m4, o4) = w(7.0, 8.0);
            println!(
                "delay {delay_ms:>3} ms | steady {:.1} dB | last second {:.1} dB | tail after reference end {:.1} dB (out rms {:.5}) | later {:.1} dB",
                db(m1, o1), db(m2, o2), db(m3, o3), o3, db(m4, o4)
            );
        }
    }

    /// Exploratory: a short reply after a long one, when the output delay differs between the two (a device
    /// that starts playing late, or a different output).
    #[test]
    #[ignore = "exploratory measurement; prints, asserts nothing"]
    fn probe_a_short_reply_with_a_different_delay() {
        for (nf, second_delay_ms) in [5usize, 12, 24].iter().flat_map(|nf| [100usize, 300, 600, 900].map(|d| (*nf, d))) {
            let warm = speechlike(1, 6, 130.0);
            let short = speechlike(5, 2, 150.0);
            let mut render = warm.clone();
            render.extend(vec![0.0f32; SR * 3]);
            let short_start = render.len();
            render.extend(short.iter().copied());
            render.extend(vec![0.0f32; SR * 3]);
            let taps: [(usize, f32); 4] = [(0, 1.0), (SR / 200, 0.5), (SR / 90, 0.3), (SR / 40, 0.15)];
            let echo_at = |i: usize| -> f32 {
                let d = if i < short_start { 100 } else { second_delay_ms } * SR / 1000;
                taps.iter().filter(|(t, _)| i >= d + t).map(|(t, g)| 0.5 * g * render[i - d - t]).sum()
            };
            let mic: Vec<f32> = (0..render.len()).map(echo_at).collect();
            let mut cfg = relaxed_config(4.0);
            cfg.delay.num_filters = nf;
            let out = run_with(EchoCanceller::with_config(cfg).expect("canceller"), &render, &mic);
            let f = |a: f32, b: f32| (a * SR as f32) as usize..(b * SR as f32) as usize;
            let t0 = short_start as f32 / SR as f32;
            let first_half = f(t0, t0 + 1.0);
            let second_half = f(t0 + 1.0, t0 + 2.0);
            let tail = f(t0 + 2.0, t0 + 2.0 + second_delay_ms as f32 / 1000.0 + 0.2);
            let d = |r: std::ops::Range<usize>| db(rms(&mic[r.clone()]), rms(&out[r]));
            let peak_tail = out[tail.clone()].iter().fold(0f32, |m, v| m.max(v.abs()));
            println!(
                "num_filters {nf:>2} | second reply delay {second_delay_ms:>3} ms | first second {:.1} dB | second second {:.1} dB | tail {:.1} dB (peak out {:.4})",
                d(first_half), d(second_half), d(tail), peak_tail
            );
        }
    }

}
