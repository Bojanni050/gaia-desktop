//! Synthetic echo-cancellation test: far-end (Gaia's voice) leaks into the mic through a
//! delayed, decaying "room" path; optionally the user talks over it (double talk).
//! Reports ERLE (echo removed, dB) in far-end-only and how much near-end energy survives.
use sonora::config::EchoCanceller;
use sonora::{AudioProcessing, Config, StreamConfig};

const SR: usize = 48_000;
const FRAME: usize = SR / 100;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
    }
}

/// Speech-like: harmonics of a wandering pitch, ~4 Hz syllable envelope, plus a little noise.
fn speechlike(seed: u64, secs: usize, f0: f32) -> Vec<f32> {
    let mut r = Lcg(seed);
    let mut out = Vec::with_capacity(SR * secs);
    let (mut ph, mut env, mut pitch) = (0f32, 0f32, f0);
    for i in 0..SR * secs {
        if i % (SR / 4) == 0 { env = if r.next() > -0.3 { 0.6 + 0.4 * r.next().abs() } else { 0.0 }; }
        if i % (SR / 50) == 0 { pitch = f0 * (1.0 + 0.15 * r.next()); }
        ph += 2.0 * std::f32::consts::PI * pitch / SR as f32;
        let mut s = 0.0;
        for h in 1..=8 { s += (ph * h as f32).sin() / h as f32; }
        out.push(0.25 * env * s + 0.01 * r.next());
    }
    out
}

fn rms(x: &[f32]) -> f32 { (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt() }
fn db(a: f32, b: f32) -> f32 { 20.0 * (a.max(1e-9) / b.max(1e-9)).log10() }

pub fn run(delay_ms: usize, gain: f32, double_talk: bool) {
    let secs = 20;
    let far = speechlike(1, secs, 130.0);
    let near_full = speechlike(2, secs, 210.0);
    // near-end talks only from 12 s to 16 s when double_talk
    let near: Vec<f32> = (0..far.len())
        .map(|i| if double_talk && i >= 12 * SR && i < 16 * SR { near_full[i] } else { 0.0 })
        .collect();
    // echo path: direct + a few decaying reflections
    let taps: [(usize, f32); 4] = [(0, 1.0), (SR / 200, 0.5), (SR / 90, 0.3), (SR / 40, 0.15)];
    let d = delay_ms * SR / 1000;
    let mic: Vec<f32> = (0..far.len())
        .map(|i| {
            let mut e = 0.0;
            for (t, g) in taps { if i >= d + t { e += g * far[i - d - t]; } }
            gain * e + near[i]
        })
        .collect();
    let echo_only: Vec<f32> = (0..far.len())
        .map(|i| mic[i] - near[i]).collect();

    let cfg = Config { echo_canceller: Some(EchoCanceller::default()), ..Default::default() };
    let mut apm = AudioProcessing::builder()
        .config(cfg)
        .capture_config(StreamConfig::new(SR as u32, 1))
        .render_config(StreamConfig::new(SR as u32, 1))
        .build();
    let _ = apm.set_stream_delay_ms(0);

    let mut out = vec![0f32; far.len()];
    let mut dest = vec![0f32; FRAME];
    let mut rdest = vec![0f32; FRAME];
    for f in 0..far.len() / FRAME {
        let r = &far[f * FRAME..(f + 1) * FRAME];
        let m = &mic[f * FRAME..(f + 1) * FRAME];
        apm.process_render_f32(&[r], &mut [&mut rdest]).unwrap();
        apm.process_capture_f32(&[m], &mut [&mut dest]).unwrap();
        out[f * FRAME..(f + 1) * FRAME].copy_from_slice(&dest);
    }
    let seg = |a: usize, b: usize, x: &[f32]| rms(&x[a * SR..b * SR]);
    println!("delay={delay_ms}ms gain={gain} double_talk={double_talk}");
    for (a, b) in [(0, 2), (2, 5), (5, 10)] {
        println!("  far-end only {a:>2}-{b:>2}s: ERLE {:>5.1} dB", db(seg(a, b, &mic), seg(a, b, &out)));
    }
    if double_talk {
        let resid = rms(&(12 * SR..16 * SR).map(|i| out[i] - near[i]).collect::<Vec<_>>());
        println!("  double talk 12-16s: near-end rms {:.4}, output rms {:.4}, error vs clean near-end {:.1} dB",
            seg(12, 16, &near), seg(12, 16, &out), db(resid, seg(12, 16, &near)));
        let (mut sxy, mut sxx, mut syy) = (0f64, 0f64, 0f64);
        for i in 12 * SR..16 * SR { let (x, y) = (near[i] as f64, out[i] as f64); sxy += x * y; sxx += x * x; syy += y * y; }
        println!("  near-end/output correlation {:.3}", sxy / (sxx.sqrt() * syy.sqrt()));
        let mut best = (0i32, 0f64);
        for lag in -960i32..=960 {
            let (mut a, mut b, mut c) = (0f64, 0f64, 0f64);
            for i in 12 * SR..16 * SR { let j = (i as i32 + lag) as usize; let (x, y) = (near[i] as f64, out[j] as f64); a += x * y; b += x * x; c += y * y; }
            let r = a / (b.sqrt() * c.sqrt());
            if r > best.1 { best = (lag, r); }
        }
        println!("  best lagged correlation {:.3} at lag {} samples ({:.1} ms)", best.1, best.0, best.0 as f32 * 1000.0 / SR as f32);
        println!("  (echo-only energy in that window {:.4})", seg(12, 16, &echo_only));
    }
}
