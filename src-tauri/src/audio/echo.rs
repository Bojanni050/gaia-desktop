//! Echo cancellation: removing Gaia's own voice from what the microphone hears, so she does not answer
//! herself and so the user can interrupt her.
//!
//! The canceller is fed in lock step: for every 10 ms of microphone audio it also gets the 10 ms of
//! what Gaia was *playing* (silence when she is quiet), and returns the microphone audio with her voice
//! taken out. It is AEC3, the WebRTC echo canceller, via the pure-Rust `aec3` crate, with the suppressor
//! relaxed four-fold: the Phase 0 measurements on the owner's speakers showed the default suppression
//! muting the user's own speech while they talked over Gaia, and the relaxed setting trading a few dB of
//! echo removal for noticeably more of the user's voice getting through.

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
        let format = AudioFormat::ten_ms(16_000, 1);
        let pipeline = linear::builder(format, format)
            .initial_delay_ms(INITIAL_DELAY_MS)
            .aec3_config(relaxed_config(4.0))
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
        let mut aec = EchoCanceller::new().expect("canceller");
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

    #[test]
    fn a_fresh_canceller_never_panics_on_odd_input() {
        let mut aec = EchoCanceller::new().unwrap();
        let mut out = vec![0.0f32; BLOCK];
        aec.process(&[0.0; BLOCK], &[1.0; BLOCK], &mut out);
        aec.process(&[1.0; BLOCK], &[-1.0; BLOCK], &mut out);
        assert!(out.iter().all(|v| v.is_finite()));
    }
}
