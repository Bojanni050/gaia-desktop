//! Real-room echo test (Phase 0 spike, throwaway).
//!
//!   echo-test [reference.wav] [--save]   play a reference through the default speakers while recording the
//!                                        default mic, run sonora AEC offline, print the numbers
//!   echo-selftest                        same analysis on a synthetic room (no audio I/O)
//!
//! Script (about 22 s): 0-10 s Gaia "speaks" and you stay silent; at 12 s a NOW prompt appears and you talk over her
//! until 19 s. Nothing is written to disk unless you pass --save (then 16 kHz mono wavs go to recordings/echo/).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const SR: usize = 16_000;
const TOTAL_SECS: usize = 21;
const FRAME10: usize = SR / 100;
const FRAME20: usize = SR / 50;
/// Remaining delay left between render and capture after alignment (what a delay hint would carry).
const RESIDUAL_DELAY_MS: usize = 40;

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}
fn db(a: f32, b: f32) -> f32 {
    20.0 * (a.max(1e-9) / b.max(1e-9)).log10()
}
fn seg(x: &[f32], a: f32, b: f32) -> &[f32] {
    let (s, e) = ((a * SR as f32) as usize, ((b * SR as f32) as usize).min(x.len()));
    &x[s.min(e)..e]
}
fn frame_rms(x: &[f32]) -> Vec<f32> {
    x.chunks_exact(FRAME20).map(rms).collect()
}
fn pct(v: &[f32], p: f32) -> f32 {
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s.get(((s.len() as f32 - 1.0) * p) as usize).copied().unwrap_or(0.0)
}

fn read_wav16(path: &Path) -> Vec<f32> {
    let mut r = hound::WavReader::open(path).expect("open reference wav");
    assert!(r.spec().sample_rate == 16_000 && r.spec().channels == 1, "reference must be 16 kHz mono (use a clip from recordings/)");
    r.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

fn write_wav16(path: &Path, x: &[f32]) {
    let spec = hound::WavSpec { channels: 1, sample_rate: 16_000, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).unwrap();
    for s in x {
        w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
    }
    w.finalize().unwrap();
}

/// Speech-like synthetic reference (used when no reference wav is available).
fn synthetic_ref(secs: usize) -> Vec<f32> {
    let mut seed = 12345u64;
    let mut rnd = move || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    };
    let (mut ph, mut env, mut pitch) = (0f32, 0f32, 130f32);
    (0..SR * secs)
        .map(|i| {
            if i % (SR / 4) == 0 {
                env = if rnd() > -0.3 { 0.6 + 0.4 * rnd().abs() } else { 0.0 };
            }
            if i % (SR / 50) == 0 {
                pitch = 130.0 * (1.0 + 0.15 * rnd());
            }
            ph += 2.0 * std::f32::consts::PI * pitch / SR as f32;
            let s: f32 = (1..=8).map(|h| (ph * h as f32).sin() / h as f32).sum();
            0.25 * env * s + 0.01 * rnd()
        })
        .collect()
}

/// Picks the most continuous, loud 21 s of the TV recording as stand-in "Gaia voice"; synthetic if absent.
fn default_reference() -> (Vec<f32>, String) {
    let p = Path::new("recordings/bgs/bgs_001.wav");
    if !p.exists() {
        return (synthetic_ref(TOTAL_SECS), "synthetic speech-like signal".into());
    }
    let all = read_wav16(p);
    let win = TOTAL_SECS * SR;
    let mut best = (0usize, -1f32);
    let mut start = 0;
    while start + win <= all.len().min(8 * 60 * SR) {
        let fr = frame_rms(&all[start..start + win]);
        let mean = fr.iter().sum::<f32>() / fr.len() as f32;
        let active = fr.iter().filter(|r| **r > 0.02).count() as f32 / fr.len() as f32;
        if mean * active > best.1 {
            best = (start, mean * active);
        }
        start += SR;
    }
    let mut seg = all[best.0..best.0 + win].to_vec();
    let peak = seg.iter().fold(0f32, |a, s| a.max(s.abs())).max(1e-6);
    seg.iter_mut().for_each(|s| *s *= 0.5 / peak);
    (seg, format!("{:.0}s-{:.0}s of recordings/bgs/bgs_001.wav", best.0 as f32 / SR as f32, (best.0 + win) as f32 / SR as f32))
}

/// Echo delay (samples at 16 kHz) such that mic[n] ~ ref[n - lag], by cross-correlation on an 8 kHz decimation.
fn estimate_lag(mic: &[f32], reference: &[f32]) -> usize {
    let dec = |x: &[f32]| -> Vec<f32> { x.chunks_exact(2).map(|c| (c[0] + c[1]) * 0.5).collect() };
    let (m, r) = (dec(mic), dec(reference));
    let (r0, r1) = (2 * 8000, 10 * 8000); // reference 2-10 s (user silent)
    let max_lag = 8000 * 3 / 2;
    let mut best = (0usize, f32::MIN);
    for lag in 0..max_lag {
        if r1 + lag > m.len() {
            break;
        }
        let c: f32 = (r0..r1).map(|i| r[i] * m[i + lag]).sum();
        if c > best.1 {
            best = (lag, c);
        }
    }
    // refine at 16 kHz
    let centre = best.0 * 2;
    let mut fine = (centre, f32::MIN);
    for lag in centre.saturating_sub(4)..=centre + 4 {
        if 10 * SR + lag > mic.len() {
            continue;
        }
        let c: f32 = (2 * SR..10 * SR).map(|i| reference[i] * mic[i + lag]).sum();
        if c > fine.1 {
            fine = (lag, c);
        }
    }
    fine.0
}

pub struct Report {
    pub lag_ms: f32,
    pub erle_db: f32,
    pub residual_db_vs_mic: f32,
    pub raw: (f32, f32),
    pub aec: (f32, f32),
    pub dt_rms_ratio_aec: f32,
    pub dt_rms_ratio_raw: f32,
}

/// `mic` starts at capture start; `reference` starts at playback start. Returns the report plus (aligned mic, processed).
pub fn analyze(mic: &[f32], reference: &[f32], save: Option<&Path>) -> Report {
    let lag = estimate_lag(mic, reference);
    let delay = RESIDUAL_DELAY_MS * SR / 1000;
    // aligned[i] = mic[i + lag - delay]: the echo of ref[i] lands at aligned[i + delay]
    let aligned: Vec<f32> = (0..reference.len())
        .map(|i| {
            let j = i as isize + lag as isize - delay as isize;
            if j >= 0 && (j as usize) < mic.len() { mic[j as usize] } else { 0.0 }
        })
        .collect();

    let cfg = Config { echo_canceller: Some(EchoCanceller::default()), ..Default::default() };
    let mut apm = AudioProcessing::builder()
        .config(cfg)
        .capture_config(StreamConfig::new(SR as u32, 1))
        .render_config(StreamConfig::new(SR as u32, 1))
        .build();
    let _ = apm.set_stream_delay_ms(RESIDUAL_DELAY_MS as i32);
    let mut out = vec![0f32; aligned.len()];
    let (mut dest, mut rdest) = (vec![0f32; FRAME10], vec![0f32; FRAME10]);
    for f in 0..aligned.len() / FRAME10 {
        let r = &reference[f * FRAME10..(f + 1) * FRAME10];
        let m = &aligned[f * FRAME10..(f + 1) * FRAME10];
        apm.process_render_f32(&[r], &mut [&mut rdest]).unwrap();
        apm.process_capture_f32(&[m], &mut [&mut dest]).unwrap();
        out[f * FRAME10..(f + 1) * FRAME10].copy_from_slice(&dest);
    }
    if let Some(dir) = save {
        std::fs::create_dir_all(dir).unwrap();
        write_wav16(&dir.join("mic_aligned.wav"), &aligned);
        write_wav16(&dir.join("aec_out.wav"), &out);
        write_wav16(&dir.join("reference.wav"), reference);
    }

    // far-end-only echo reduction, after convergence
    let erle = db(rms(seg(&aligned, 5.0, 10.0)), rms(seg(&out, 5.0, 10.0)));
    let residual = db(rms(seg(&out, 5.0, 10.0)), rms(seg(&aligned, 5.0, 10.0)));
    // barge-in detectability: threshold = 3 x p95 of frame rms in 5-8 s (far-end only);
    // false rate on 8-10 s, hit rate on the 13-19 s double-talk window
    let rate = |x: &[f32]| -> (f32, f32) {
        // floor 0.005 (-46 dBFS): after AEC the settled residual is near digital silence, so 3 x p95 alone would be absurdly low
        let thr = (3.0 * pct(&frame_rms(seg(x, 5.0, 8.0)), 0.95)).max(0.005);
        let above = |a: f32, b: f32| {
            let fr = frame_rms(seg(x, a, b));
            fr.iter().filter(|r| **r > thr).count() as f32 / fr.len().max(1) as f32
        };
        (above(8.0, 10.0), above(13.0, 19.0))
    };
    let (raw, aec) = (rate(&aligned), rate(&out));
    Report {
        lag_ms: lag as f32 * 1000.0 / SR as f32,
        erle_db: erle,
        residual_db_vs_mic: residual,
        raw,
        aec,
        dt_rms_ratio_aec: db(rms(seg(&out, 13.0, 19.0)), rms(seg(&out, 5.0, 10.0))),
        dt_rms_ratio_raw: db(rms(seg(&aligned, 13.0, 19.0)), rms(seg(&aligned, 5.0, 10.0))),
    }
}

fn print_report(r: &Report) {
    println!("\n=== echo test result ===");
    println!("total render->mic lag (incl. start offset): {:.0} ms", r.lag_ms);
    println!("echo removed, far-end only 5-10 s (ERLE): {:.1} dB   (residual {:.1} dB vs raw mic)", r.erle_db, r.residual_db_vs_mic);
    println!("barge-in detection with threshold = max(3 x p95 of the settled far-end-only frames, 0.005):");
    println!("                      false trigger (8-10 s)   hit rate while you talk (13-19 s)");
    println!("  raw mic                   {:>5.0}%                    {:>5.0}%", r.raw.0 * 100.0, r.raw.1 * 100.0);
    println!("  after AEC                 {:>5.0}%                    {:>5.0}%", r.aec.0 * 100.0, r.aec.1 * 100.0);
    println!("speech-over-echo level while talking vs far-end-only: raw {:+.1} dB, after AEC {:+.1} dB", r.dt_rms_ratio_raw, r.dt_rms_ratio_aec);
    let good = r.erle_db >= 20.0 && r.aec.0 <= 0.05 && r.aec.1 >= 0.6;
    println!("verdict: {}", if good { "PROMISING (ERLE >= 20 dB, false triggers <= 5%, hit rate >= 60%)" } else { "NOT YET: see numbers above (targets: ERLE >= 20 dB, false triggers <= 5%, hit rate >= 60%)" });
}

pub fn selftest() {
    // synthetic room: 90 ms delay, multi-tap path, user talks 12-19 s
    let reference = synthetic_ref(TOTAL_SECS);
    let user = {
        let r = synthetic_ref(TOTAL_SECS + 3);
        r.iter().skip(SR).map(|v| v * 1.4).collect::<Vec<f32>>()
    };
    let start_offset = SR / 2; // capture starts 0.5 s before playback
    let delay = 90 * SR / 1000;
    let taps: [(usize, f32); 3] = [(0, 0.5), (SR / 200, 0.25), (SR / 90, 0.12)];
    let mic: Vec<f32> = (0..start_offset + reference.len() + SR)
        .map(|n| {
            let mut e = 0.0;
            for (t, g) in taps {
                let k = n as isize - (start_offset + delay + t) as isize;
                if k >= 0 && (k as usize) < reference.len() { e += g * reference[k as usize]; }
            }
            let t = n as f32 / SR as f32 - 0.5;
            let u = if (12.0..19.0).contains(&t) { user[n] } else { 0.0 };
            e + u + 0.003 * ((n * 7919 % 1000) as f32 / 500.0 - 1.0)
        })
        .collect();
    println!("selftest: true start offset 500 ms + 90 ms path = 590 ms lag expected");
    print_report(&analyze(&mic, &reference, None));
}

pub fn run(ref_path: Option<String>, save: bool, label: Option<String>) {
    let (reference, ref_desc) = match ref_path {
        Some(p) => (read_wav16(Path::new(&p)), p),
        None => default_reference(),
    };
    assert!(reference.len() >= TOTAL_SECS * SR, "reference must be at least {TOTAL_SECS} s");
    let reference = reference[..TOTAL_SECS * SR].to_vec();
    println!("reference: {ref_desc}");

    let host = cpal::default_host();
    let input = host.default_input_device().expect("no default input");
    let output = host.default_output_device().expect("no default output");
    let icfg = input.default_input_config().expect("input config");
    let ocfg = output.default_output_config().expect("output config");
    assert!(icfg.sample_format() == SampleFormat::F32 && ocfg.sample_format() == SampleFormat::F32, "spike handles f32 devices only");
    let (irate, ich) = (icfg.sample_rate(), icfg.channels() as usize);
    let (orate, och) = (ocfg.sample_rate() as usize, ocfg.channels() as usize);
    println!("mic: {:?}  | speakers: {:?}", input.description().map(|d| d.name().to_string()), output.description().map(|d| d.name().to_string()));

    // reference upsampled (linear) to the speaker rate, written to all channels
    let up: Vec<f32> = {
        let n = reference.len() * orate / SR;
        let mut v = Vec::with_capacity(n * och);
        for k in 0..n {
            let pos = k as f64 * SR as f64 / orate as f64;
            let i = pos as usize;
            let fr = (pos - pos.floor()) as f32;
            let a = reference[i.min(reference.len() - 1)];
            let b = reference[(i + 1).min(reference.len() - 1)];
            let s = a + (b - a) * fr;
            for _ in 0..och { v.push(s); }
        }
        v
    };
    let go = Arc::new(AtomicBool::new(false));
    let g2 = go.clone();
    let mut pos = 0usize;
    let out_stream = output
        .build_output_stream(
            ocfg.into(),
            move |data: &mut [f32], _: &_| {
                for s in data.iter_mut() {
                    *s = if g2.load(Ordering::Relaxed) && pos < up.len() { let v = up[pos]; pos += 1; v } else { 0.0 };
                }
            },
            |e: cpal::Error| eprintln!("output error: {e}"),
            None,
        )
        .expect("output stream");
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let xr = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let x2 = xr.clone();
    let in_stream = input
        .build_input_stream(
            icfg.into(),
            move |d: &[f32], _: &_| { let _ = tx.send(d.to_vec()); },
            move |e: cpal::Error| { x2.fetch_add(1, Ordering::Relaxed); eprintln!("input error: {e}"); },
            None,
        )
        .expect("input stream");

    println!("\nSet your speaker volume to a NORMAL listening level, stay in your usual seat, keep the room as it is.");
    println!("Starting in 5 s. Stay SILENT until you see NOW.");
    std::thread::sleep(Duration::from_secs(5));
    let t_cap = Instant::now();
    in_stream.play().expect("input play");
    out_stream.play().expect("output play");
    std::thread::sleep(Duration::from_millis(500));
    let issue = Instant::now();
    let issue_offset_ms = issue.duration_since(t_cap).as_millis();
    go.store(true, Ordering::Relaxed);
    println!("Gaia is speaking... (silent please)");
    let mut captured: Vec<f32> = Vec::new();
    let drain = |rx: &std::sync::mpsc::Receiver<Vec<f32>>, buf: &mut Vec<f32>, until: Instant| {
        while Instant::now() < until {
            if let Ok(c) = rx.recv_timeout(Duration::from_millis(20)) { buf.extend_from_slice(&c); }
        }
    };
    drain(&rx, &mut captured, issue + Duration::from_secs(12));
    println!(">>> NOW: talk over her (count 1, 2, 3... out loud) <<<");
    drain(&rx, &mut captured, issue + Duration::from_secs(19));
    println!(">>> stop talking <<<");
    drain(&rx, &mut captured, issue + Duration::from_secs(TOTAL_SECS as u64 + 1));
    drop(in_stream);
    drop(out_stream);
    while let Ok(c) = rx.try_recv() { captured.extend_from_slice(&c); }

    let glitches = xr.load(Ordering::Relaxed);
    let mic: Vec<f32> = crate::harness::to_16k_mono_pub(&captured, irate, ich).iter().map(|s| *s as f32 / 32768.0).collect();
    println!("captured {:.1} s, {glitches} audio glitch(es); capture started {issue_offset_ms} ms before playback was issued", mic.len() as f32 / SR as f32);
    let save_dir = std::path::PathBuf::from(format!(
        "recordings/echo/{}{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        label.map(|l| format!("_{l}")).unwrap_or_default()
    ));
    let report = analyze(&mic, &reference, if save { Some(save_dir.as_path()) } else { None });
    println!("(system latency estimate: lag minus issue offset = {:.0} ms, +/- ~30 ms)", report.lag_ms - issue_offset_ms as f32);
    print_report(&report);
    if save { println!("saved mic_aligned.wav, aec_out.wav, reference.wav in recordings/echo/<timestamp>/ (gitignored: it is your voice)"); }
}
