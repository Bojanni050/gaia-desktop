//! Recording + scoring harness for the wake-word spike.
//!
//!   collect <label> <count> [secs] [device-id]   record labelled 16 kHz mono clips
//!   score [n_refs]                               build a Rustpotter ref from `hey` clips, score the rest
//!
//! Clips go to ./recordings/<label>/ (gitignored: it is your voice, never commit or upload it).
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use rustpotter::{
    AudioFmt, DetectorConfig, Endianness, Rustpotter, RustpotterConfig, SampleFormat as RpFmt,
    WakewordRef, WakewordRefBuildFromFiles,
};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const TARGET: u32 = 16_000;

fn dir(label: &str) -> PathBuf {
    Path::new("recordings").join(label)
}

fn prompt_for(label: &str) -> &'static str {
    match label {
        "hey" => "say: \"Hey Gaia\"",
        "neg" => "say ANY OTHER phrase (\"hé jij daar\", \"Gaia\", \"hey Maya\", a sentence)",
        "bg" => "stay silent, or play TV/music/talk normally (ambient false-accept test)",
        _ => "speak",
    }
}

/// Mono-ise, box-filter, then linearly resample to 16 kHz.
fn to_16k_mono(samples: &[f32], rate: u32, channels: usize) -> Vec<i16> {
    let mono: Vec<f32> = samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect();
    if mono.is_empty() {
        return Vec::new();
    }
    let w = (rate as f32 / TARGET as f32).round().max(1.0) as usize;
    let filt: Vec<f32> = if w > 1 {
        (0..mono.len())
            .map(|i| {
                let a = i.saturating_sub(w / 2);
                let b = (i + w / 2 + 1).min(mono.len());
                mono[a..b].iter().sum::<f32>() / (b - a) as f32
            })
            .collect()
    } else {
        mono
    };
    let n = (filt.len() as f64 * TARGET as f64 / rate as f64) as usize;
    (0..n)
        .map(|k| {
            let pos = k as f64 * rate as f64 / TARGET as f64;
            let i = pos as usize;
            let fr = (pos - pos.floor()) as f32;
            let a = filt[i.min(filt.len() - 1)];
            let b = filt[(i + 1).min(filt.len() - 1)];
            ((a + (b - a) * fr).clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

fn write_wav(path: &Path, pcm: &[i16]) {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: TARGET,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec).expect("create wav");
    for s in pcm {
        w.write_sample(*s).unwrap();
    }
    w.finalize().unwrap();
}

fn read_wav(path: &Path) -> Vec<i16> {
    hound::WavReader::open(path)
        .expect("open wav")
        .samples::<i16>()
        .map(|s| s.unwrap())
        .collect()
}

pub fn collect(label: &str, count: usize, secs: f32, device_id: Option<String>) {
    let host = cpal::default_host();
    let device = match device_id {
        Some(s) => host.device_by_id(&s.parse().expect("bad id")).expect("no such device"),
        None => host.default_input_device().expect("no default input"),
    };
    let cfg = device.default_input_config().expect("config");
    assert!(cfg.sample_format() == SampleFormat::F32, "only f32 input handled in the spike");
    let (rate, ch) = (cfg.sample_rate(), cfg.channels() as usize);
    println!(
        "mic: {:?}  ({rate} Hz, {ch} ch)",
        device.description().map(|d| d.name().to_string())
    );
    let out = dir(label);
    std::fs::create_dir_all(&out).unwrap();
    let start = std::fs::read_dir(&out).map(|d| d.count()).unwrap_or(0);

    let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let b2 = buf.clone();
    let stream = device
        .build_input_stream(
            cfg.into(),
            move |d: &[f32], _: &_| b2.lock().unwrap().extend_from_slice(d),
            |e: cpal::Error| eprintln!("stream error: {e}"),
            None,
        )
        .expect("stream");
    stream.play().expect("play");
    std::thread::sleep(Duration::from_millis(300));

    println!("{}\nrecording {count} clip(s) of {secs}s into {}", prompt_for(label), out.display());
    for i in 0..count {
        for n in (1..=3).rev() {
            print!("  [{}/{count}] {n}.. ", i + 1);
            std::io::stdout().flush().ok();
            std::thread::sleep(Duration::from_millis(700));
        }
        buf.lock().unwrap().clear();
        println!("NOW");
        std::thread::sleep(Duration::from_secs_f32(secs));
        let taken: Vec<f32> = std::mem::take(&mut *buf.lock().unwrap());
        let pcm = to_16k_mono(&taken, rate, ch);
        let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
        let path = out.join(format!("{label}_{:03}.wav", start + i + 1));
        write_wav(&path, &pcm);
        println!(
            "      saved {} ({} samples, peak {:.2}){}",
            path.display(),
            pcm.len(),
            peak as f32 / 32768.0,
            if peak < 800 { "  <-- very quiet, check the mic" } else { "" }
        );
    }
    drop(stream);
}

fn wavs(label: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir(label))
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().map(|x| x == "wav").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Runs a clip (plus 1.5 s of silence to flush) through the detector; returns (detections, best score).
fn detect(det: &mut Rustpotter, pcm: &[i16]) -> (usize, f32) {
    det.reset();
    let n = det.get_samples_per_frame();
    // Leading silence too: the detector's window must be able to slide across the word, as it does in a live stream.
    let mut padded = vec![0i16; TARGET as usize];
    padded.extend_from_slice(pcm);
    padded.extend(std::iter::repeat(0i16).take(TARGET as usize * 3 / 2));
    let (mut hits, mut best) = (0usize, 0f32);
    for frame in padded.chunks_exact(n) {
        if let Some(d) = det.process_samples::<i16>(frame.to_vec()) {
            hits += 1;
            best = best.max(d.score);
        }
        if let Some(p) = det.get_partial_detection() {
            best = best.max(p.score);
        }
    }
    (hits, best)
}

fn detector(refs: &[PathBuf], threshold: f32, avg: f32) -> Rustpotter {
    let cfg = RustpotterConfig {
        fmt: AudioFmt {
            sample_rate: TARGET as usize,
            sample_format: RpFmt::I16,
            channels: 1,
            endianness: Endianness::Little,
        },
        detector: DetectorConfig { threshold, avg_threshold: avg, ..DetectorConfig::default() },
        ..RustpotterConfig::default()
    };
    let mut d = Rustpotter::new(&cfg).expect("rustpotter");
    let w = WakewordRef::new_from_sample_files(
        "hey_gaia".into(),
        None,
        None,
        refs.iter().map(|p| p.to_string_lossy().to_string()).collect(),
        16,
    )
    .expect("build wakeword ref");
    d.add_wakeword_ref("hey_gaia", w).expect("add ref");
    d
}

pub fn score(n_refs: usize) {
    let hey = wavs("hey");
    let neg = wavs("neg");
    let bg = wavs("bg");
    println!("clips: hey={} neg={} bg={}", hey.len(), neg.len(), bg.len());
    if hey.len() < n_refs + 2 {
        eprintln!("need at least {} `hey` clips (n_refs {n_refs} + 2 to test)", n_refs + 2);
        return;
    }
    let bg_secs: f32 = bg.iter().map(|p| read_wav(p).len() as f32 / TARGET as f32).sum::<f32>().abs();
    // Two folds: refs = first n_refs / last n_refs; test on the rest. Refs are never tested on themselves.
    let folds: Vec<(Vec<PathBuf>, Vec<PathBuf>)> = vec![
        (hey[..n_refs].to_vec(), hey[n_refs..].to_vec()),
        (hey[hey.len() - n_refs..].to_vec(), hey[..hey.len() - n_refs].to_vec()),
    ];
    // Harness self-check: GAIA_SELFTEST=1 scores the reference clips themselves (must hit).
    let folds = if std::env::var("GAIA_SELFTEST").is_ok() {
        folds.into_iter().map(|(r, _)| (r.clone(), r)).collect()
    } else {
        folds
    };
    println!(
        "{:>9} {:>9} | {:>8} {:>8} | {:>8} {:>12}",
        "threshold", "avg_thr", "hey hit", "hey miss", "neg FP", "bg FP/hour"
    );
    for (thr, avg) in [(0.30, 0.15), (0.40, 0.20), (0.50, 0.25), (0.60, 0.30), (0.70, 0.35)] {
        let (mut hit, mut miss, mut nfp, mut bfp) = (0usize, 0usize, 0usize, 0usize);
        for (refs, tests) in &folds {
            let mut d = detector(refs, thr, avg);
            for t in tests {
                let (h, b) = detect(&mut d, &read_wav(t));
                if std::env::var("GAIA_TRACE").is_ok() {
                    println!("    {} -> detections {h}, best score {b:.3}", t.display());
                }
                if h > 0 {
                    hit += 1
                } else {
                    miss += 1
                }
            }
            for t in &neg {
                if detect(&mut d, &read_wav(t)).0 > 0 {
                    nfp += 1
                }
            }
            for t in &bg {
                bfp += detect(&mut d, &read_wav(t)).0;
            }
        }
        let nf = folds.len();
        let per_hour = if bg_secs > 0.0 {
            format!("{:.1}", bfp as f32 / nf as f32 / bg_secs * 3600.0)
        } else {
            "n/a".into()
        };
        println!("{thr:>9.2} {avg:>9.2} | {hit:>8} {miss:>8} | {:>8} {per_hour:>12}", nfp / nf);
    }
    println!(
        "(hey hit/miss summed over {} folds; neg FP averaged per fold; bg = {bg_secs:.0}s of ambient audio)",
        folds.len()
    );
}
