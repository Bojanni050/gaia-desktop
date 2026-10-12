//! Offline replay of saved echo-test runs through several echo-canceller variants (Phase 0 spike, throwaway).
//!
//!   echo-replay [dir ...]    default: recordings/echo and every recordings/echo/<timestamp>/ that holds
//!                            reference.wav + mic_aligned.wav (written by `echo-test --save`)
//!
//! `mic_aligned.wav` is already shifted so that about 40 ms of delay remains between render and capture,
//! so every variant gets the same 40 ms delay hint. "converged" variants run the recording twice and score
//! the second pass (the filter has already seen this echo path, as in a long conversation).
use aec3::api::config::{DominantNearendDetection, EchoCanceller3Config, MaskingThresholds, Tuning};
use aec3::nodes::audio::AudioFormat;
use aec3::pipelines::linear;
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};
use std::path::{Path, PathBuf};

const SR: usize = 16_000;
const FRAME10: usize = SR / 100;
const FRAME20: usize = SR / 50;
const FLOOR: f32 = 0.005;

fn read(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path).expect("open wav").samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}
fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}
fn seg(x: &[f32], a: f32, b: f32) -> &[f32] {
    let (s, e) = ((a * SR as f32) as usize, ((b * SR as f32) as usize).min(x.len()));
    &x[s.min(e)..e]
}
fn frames(x: &[f32]) -> Vec<f32> {
    x.chunks_exact(FRAME20).map(rms).collect()
}
fn p95(v: &[f32]) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s.get(((s.len() as f32 - 1.0) * 0.95) as usize).copied().unwrap_or(0.0)
}

pub(crate) fn sonora_pass(reference: &[f32], mic: &[f32]) -> Vec<f32> {
    let cfg = Config { echo_canceller: Some(EchoCanceller::default()), ..Default::default() };
    let mut apm = AudioProcessing::builder()
        .config(cfg)
        .capture_config(StreamConfig::new(SR as u32, 1))
        .render_config(StreamConfig::new(SR as u32, 1))
        .build();
    let _ = apm.set_stream_delay_ms(40);
    let mut out = vec![0f32; mic.len()];
    let (mut d, mut rd) = (vec![0f32; FRAME10], vec![0f32; FRAME10]);
    for f in 0..mic.len() / FRAME10 {
        let r = &reference[f * FRAME10..(f + 1) * FRAME10];
        let m = &mic[f * FRAME10..(f + 1) * FRAME10];
        apm.process_render_f32(&[r], &mut [&mut rd]).unwrap();
        apm.process_capture_f32(&[m], &mut [&mut d]).unwrap();
        out[f * FRAME10..(f + 1) * FRAME10].copy_from_slice(&d);
    }
    out
}

pub(crate) fn aec3_run(reference: &[f32], mic: &[f32], cfg: Option<EchoCanceller3Config>) -> Vec<f32> {
    let fmt = AudioFormat::ten_ms(SR as u32, 1);
    let mut b = linear::builder(fmt, fmt)
        .initial_delay_ms(40)
        .enable_noise_suppression(false)
        .enable_gain_controller2(false)
        .enable_post_filter(false);
    if let Some(c) = cfg {
        b = b.aec3_config(c);
    }
    let mut p = b.build().expect("aec3 pipeline");
    let mut out = vec![0f32; mic.len()];
    let mut dest = vec![0f32; FRAME10];
    for f in 0..mic.len() / FRAME10 {
        p.handle_render_frame(&reference[f * FRAME10..(f + 1) * FRAME10]).unwrap();
        let produced = p.process_capture_frame(&mic[f * FRAME10..(f + 1) * FRAME10], &mut dest).unwrap();
        if produced {
            out[f * FRAME10..(f + 1) * FRAME10].copy_from_slice(&dest);
        }
    }
    out
}

/// Less aggressive suppression: masking thresholds raised, nearend-dominant state entered more easily and held longer.
pub(crate) fn gentle(level: u8) -> EchoCanceller3Config {
    let mut c = EchoCanceller3Config::default();
    let k = if level == 1 { 2.0 } else { 4.0 };
    let t = |a: f32, b: f32, e: f32| MaskingThresholds::new(a, b, e);
    c.suppressor.normal_tuning = Tuning::new(t(0.3 * k, 0.4 * k, 0.3), t(0.07 * k, 0.1 * k, 0.3), 2.0, 0.25);
    c.suppressor.nearend_tuning = Tuning::new(t(1.09 * k, 1.1 * k, 0.3), t(0.1 * k, 0.3 * k, 0.3), 2.0, 0.25);
    c.suppressor.dominant_nearend_detection = DominantNearendDetection {
        enr_threshold: 0.25 * k,
        enr_exit_threshold: 10.0,
        snr_threshold: 30.0 / k,
        hold_duration: 50 * k as usize,
        trigger_threshold: 12,
        use_during_initial_phase: true,
        use_unbounded_echo_spectrum: true,
    };
    c
}

struct Row {
    name: String,
    erle: f32,
    false_rate: f32,
    hit: f32,
    talk_rms: f32,
    /// seconds after 12.0 s until 3 consecutive 20 ms frames exceed the threshold (None = never)
    onset: Option<f32>,
    /// number of such 3-frame detections between 5 s and 12 s (user silent: every one is a false barge-in)
    false_events: usize,
}

fn score(name: &str, out: &[f32], mic: &[f32]) -> Row {
    let thr = (3.0 * p95(&frames(seg(out, 5.0, 8.0)))).max(FLOOR);
    let rate = |a: f32, b: f32| {
        let f = frames(seg(out, a, b));
        f.iter().filter(|r| **r > thr).count() as f32 / f.len().max(1) as f32
    };
    let fr: Vec<bool> = frames(out).iter().map(|r| *r > thr).collect();
    let frame_s = FRAME20 as f32 / SR as f32;
    let run3 = |i: usize| i + 2 < fr.len() && fr[i] && fr[i + 1] && fr[i + 2];
    let a = (12.0 / frame_s) as usize;
    let onset = (a..fr.len()).find(|i| run3(*i)).map(|i| i as f32 * frame_s - 12.0);
    let (b0, b1) = ((5.0 / frame_s) as usize, a);
    let mut false_events = 0;
    let mut i = b0;
    while i < b1 {
        if run3(i) { false_events += 1; i += 3; } else { i += 1; }
    }
    Row {
        onset,
        false_events,
        name: name.into(),
        erle: 20.0 * (rms(seg(mic, 5.0, 10.0)).max(1e-9) / rms(seg(out, 5.0, 10.0)).max(1e-9)).log10(),
        false_rate: rate(8.0, 10.0),
        hit: rate(13.0, 19.0),
        talk_rms: rms(seg(out, 13.0, 19.0)),
    }
}

fn twice(x: &[f32]) -> Vec<f32> {
    let mut v = x.to_vec();
    v.extend_from_slice(x);
    v
}

fn replay_one(dir: &Path) {
    let (r, m) = (read(&dir.join("reference.wav")), read(&dir.join("mic_aligned.wav")));
    let n = r.len().min(m.len());
    let (r, m) = (&r[..n], &m[..n]);
    println!("\n== {} ==  (raw mic talk rms 13-19 s: {:.4})", dir.display(), rms(seg(m, 13.0, 19.0)));
    let (rr, mm) = (twice(r), twice(m));
    let second = |out: Vec<f32>| out[n..].to_vec();
    let mut rows = vec![
        score("sonora default (cold)", &sonora_pass(r, m), m),
        score("sonora default (converged)", &second(sonora_pass(&rr, &mm)), m),
        score("aec3 default (cold)", &aec3_run(r, m, None), m),
        score("aec3 default (converged)", &second(aec3_run(&rr, &mm, None)), m),
        score("aec3 gentle x2 (cold)", &aec3_run(r, m, Some(gentle(1))), m),
        score("aec3 gentle x2 (converged)", &second(aec3_run(&rr, &mm, Some(gentle(1)))), m),
        score("aec3 gentle x4 (cold)", &aec3_run(r, m, Some(gentle(2))), m),
        score("aec3 gentle x4 (converged)", &second(aec3_run(&rr, &mm, Some(gentle(2)))), m),
    ];
    println!("{:<30} {:>8} {:>8} {:>8} {:>10} {:>10} {:>9}", "variant", "ERLE dB", "false %", "hit %", "talk rms", "onset s", "false ev");
    for r in rows.drain(..) {
        let onset = r.onset.map(|o| format!("{o:.2}")).unwrap_or_else(|| "never".into());
        println!("{:<30} {:>8.1} {:>8.0} {:>8.0} {:>10.4} {:>10} {:>9}", r.name, r.erle, r.false_rate * 100.0, r.hit * 100.0, r.talk_rms, onset, r.false_events);
    }
}

pub fn run(dirs: Vec<String>) {
    let mut targets: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    if targets.is_empty() {
        let root = Path::new("recordings/echo");
        targets.push(root.to_path_buf());
        if let Ok(rd) = std::fs::read_dir(root) {
            targets.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    let mut any = false;
    for d in targets {
        if d.join("reference.wav").exists() && d.join("mic_aligned.wav").exists() {
            replay_one(&d);
            any = true;
        }
    }
    if !any {
        eprintln!("no recordings found: run `echo-test --save` first");
    }
    println!("\n(targets: ERLE >= 20 dB, false <= 5%, hit >= 60%; threshold = max(3 x p95 of settled far-end-only frames, {FLOOR}))");
}
