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
use std::sync::Arc;
use std::time::Duration;

const TARGET: u32 = 16_000;

fn dir(label: &str) -> PathBuf {
    Path::new("recordings").join(label)
}

fn prompt_for(label: &str) -> &'static str {
    match label {
        "hey" => "say: \"Hey Gaia\"",
        "neg" => "say ANY OTHER phrase (\"hé jij daar\", \"Gaia\", \"hey Maya\", a sentence)",
        "bg" | "bgs" => "stay silent, or play TV/music/talk normally (ambient false-accept test)",
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

    // The audio callback only forwards small chunks over a channel: no big-Vec reallocation, no
    // lock shared with the main thread. (A growing Vec behind a Mutex caused buffer over/underruns.)
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let xruns = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let x2 = xruns.clone();
    let stream = device
        .build_input_stream(
            cfg.into(),
            move |d: &[f32], _: &_| {
                let _ = tx.send(d.to_vec());
            },
            move |e: cpal::Error| {
                x2.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                eprintln!("stream error: {e}");
            },
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
        while rx.try_recv().is_ok() {} // drop what arrived during the countdown
        let glitches_before = xruns.load(std::sync::atomic::Ordering::Relaxed);
        println!("NOW");
        let mut taken: Vec<f32> = Vec::with_capacity((rate as f32 * ch as f32 * secs) as usize + 65_536);
        let deadline = std::time::Instant::now() + Duration::from_secs_f32(secs);
        while std::time::Instant::now() < deadline {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(50)) {
                taken.extend_from_slice(&chunk);
            }
        }
        while let Ok(chunk) = rx.try_recv() {
            taken.extend_from_slice(&chunk);
        }
        let glitches = xruns.load(std::sync::atomic::Ordering::Relaxed) - glitches_before;
        if glitches > 0 {
            println!("      WARNING: {glitches} audio glitch(es) during this clip, consider re-recording it");
        }
        let pcm = to_16k_mono(&taken, rate, ch);
        if pcm.is_empty() {
            println!("      NO AUDIO received for this clip: check the mic, then re-run");
        }
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

/// Trim to the active region (20 ms frames above 12% of the clip peak) plus a 120 ms margin.
fn trim(pcm: &[i16]) -> Vec<i16> {
    let fr = (TARGET as usize) / 50;
    let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32;
    let loud: Vec<bool> = pcm.chunks(fr).map(|c| c.iter().map(|s| s.unsigned_abs() as f32).fold(0f32, f32::max) > 0.12 * peak).collect();
    let (Some(a), Some(b)) = (loud.iter().position(|x| *x), loud.iter().rposition(|x| *x)) else { return pcm.to_vec() };
    let margin = 6usize;
    let start = a.saturating_sub(margin) * fr;
    let end = ((b + 1 + margin) * fr).min(pcm.len());
    pcm[start..end].to_vec()
}

/// Writes trimmed copies of `files` to recordings/_trimmed and returns their paths.
fn trimmed_copies(files: &[PathBuf]) -> Vec<PathBuf> {
    let out = dir("_trimmed");
    std::fs::create_dir_all(&out).unwrap();
    files.iter().map(|f| {
        let p = out.join(f.file_name().unwrap());
        write_wav(&p, &trim(&read_wav(f)));
        p
    }).collect()
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
    let usable = |label: &str| -> Vec<PathBuf> {
        wavs(label)
            .into_iter()
            .filter(|p| {
                let pcm = read_wav(p);
                let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0;
                let word = trim(&pcm).len() as f32 / TARGET as f32;
                // A single "Hey Gaia" is roughly 0.5-1.5 s once trimmed; longer means noise/talking, shorter means a cut-off clip.
                let ok = pcm.len() >= TARGET as usize / 2 && peak >= 0.05 && (label.starts_with("bg") || (0.5..=1.5).contains(&word));
                if !ok {
                    println!("skipping unusable clip {} ({:.2}s, trimmed {word:.2}s, peak {peak:.2})", p.display(), pcm.len() as f32 / TARGET as f32);
                }
                ok
            })
            .collect()
    };
    let hey = usable("hey");
    let neg = usable("neg");
    let bg = usable(&std::env::var("GAIA_BG_LABEL").unwrap_or_else(|_| "bg".into()));
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
    for (thr, avg) in [(0.50, 0.25), (0.54, 0.27), (0.57, 0.28), (0.60, 0.30), (0.62, 0.31), (0.64, 0.32), (0.66, 0.33), (0.68, 0.34)] {
        let (mut hit, mut miss, mut nfp, mut bfp) = (0usize, 0usize, 0usize, 0usize);
        for (refs, tests) in &folds {
            let mut d = detector(&trimmed_copies(refs), thr, avg);
            for t in tests {
                let (h, b) = detect(&mut d, &trim(&read_wav(t)));
                if std::env::var("GAIA_TRACE").is_ok() {
                    println!("    thr {thr:.2}: {} -> {}  (best score {b:.3})", t.file_name().unwrap().to_string_lossy(), if h > 0 { "HIT" } else { "MISS" });
                }
                if h > 0 {
                    hit += 1
                } else {
                    miss += 1
                }
            }
            for t in &neg {
                if detect(&mut d, &trim(&read_wav(t))).0 > 0 {
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

// ---------------------------------------------------------------------------------------------
// Trained-model mode: a small neural net (Rustpotter "wakeword model") instead of DTW references.
//   train [epochs=1500] [tiny|small|medium|large]   build a 70/30 split from recordings/, train, save
//   score-model                                     score the held-out 30% (never seen in training)
// Split is deterministic: every 3rd clip (index % 3 == 0) is held out. Ambient audio is cut in time:
// the first 70% trains the `none` class, the last 30% is the held-out false-accept test.
// ---------------------------------------------------------------------------------------------
use rustpotter::{ModelType, WakewordModel, WakewordModelTrain, WakewordModelTrainOptions, WakewordSave};

const MODEL_PATH: &str = "recordings/_model.rpw";

fn reset_dir(name: &str) -> PathBuf {
    let d = dir(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn train(epochs: usize, model_type: &str) {
    let (train_dir, test_dir) = (reset_dir("_train"), reset_dir("_test"));
    let (mut n_train, mut n_test) = ([0usize; 2], [0usize; 2]); // [hey, none]
    let mut put = |is_test: bool, hey: bool, idx: usize, pcm: &[i16]| {
        let tag = if hey { "[hey_gaia]" } else { "[none]" };
        let (d, n) = if is_test { (&test_dir, &mut n_test) } else { (&train_dir, &mut n_train) };
        write_wav(&d.join(format!("{}_{idx:04}{tag}.wav", if hey { "hey" } else { "none" })), pcm);
        n[if hey { 0 } else { 1 }] += 1;
    };
    for (i, p) in usable_list("hey").iter().enumerate() {
        put(i % 3 == 0, true, i, &trim(&read_wav(p)));
    }
    for (i, p) in usable_list("neg").iter().enumerate() {
        put(i % 3 == 0, false, i, &trim(&read_wav(p)));
    }
    // Ambient: 1 s windows, 50% overlap; first 70% of each file trains, the rest is held out.
    let mut k = 0usize;
    for p in usable_list("bg") {
        let pcm = read_wav(&p);
        let cut = pcm.len() * 7 / 10;
        let win = TARGET as usize;
        let mut s = 0;
        while s + win <= pcm.len() {
            put(s >= cut, false, 10_000 + k, &pcm[s..s + win]);
            k += 1;
            s += win / 2;
        }
    }
    println!("training set: {} hey + {} none; test set: {} hey + {} none", n_train[0], n_train[1], n_test[0], n_test[1]);
    let m_type: ModelType = model_type.parse().expect("model type: tiny|small|medium|large");
    let opts = WakewordModelTrainOptions::new(m_type, 0.03, epochs, (epochs / 10).max(1), 16);
    let model = WakewordModel::train_from_dirs(opts, train_dir.to_string_lossy().into(), test_dir.to_string_lossy().into(), None)
        .expect("training failed");
    model.save_to_file(MODEL_PATH).expect("save model");
    println!("saved {MODEL_PATH}");
}

/// Same filtering `score` uses, as a reusable list.
fn usable_list(label: &str) -> Vec<PathBuf> {
    wavs(label)
        .into_iter()
        .filter(|p| {
            let pcm = read_wav(p);
            let peak = pcm.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0) as f32 / 32768.0;
            let word = trim(&pcm).len() as f32 / TARGET as f32;
            pcm.len() >= TARGET as usize / 2 && peak >= 0.05 && (label.starts_with("bg") || (0.5..=1.5).contains(&word))
        })
        .collect()
}

pub fn score_model() {
    let test_dir = dir("_test");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&test_dir).expect("run `train` first").filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    let (hey, none): (Vec<_>, Vec<_>) = files.into_iter().partition(|p| p.to_string_lossy().contains("[hey_gaia]"));
    let bg: Vec<&PathBuf> = none.iter().filter(|p| p.file_name().unwrap().to_string_lossy().starts_with("none_1")).collect();
    let neg: Vec<&PathBuf> = none.iter().filter(|p| !bg.contains(p)).collect();
    // bg windows overlap 50%: each adds 0.5 s of new audio (plus 0.5 s for the first)
    let bg_secs = bg.len() as f32 * 0.5 + 0.5;
    println!("held-out: hey={} neg={} bg windows={} (~{bg_secs:.0}s)", hey.len(), neg.len(), bg.len());
    println!("{:>9} | {:>8} {:>8} | {:>8} | {:>12}", "threshold", "hey hit", "hey miss", "neg FP", "bg FP/hour");
    for thr in [0.50f32, 0.60, 0.70, 0.80, 0.90, 0.95, 0.98] {
        let cfg = RustpotterConfig {
            fmt: AudioFmt { sample_rate: TARGET as usize, sample_format: RpFmt::I16, channels: 1, endianness: Endianness::Little },
            detector: DetectorConfig { threshold: thr, avg_threshold: 0.0, ..DetectorConfig::default() },
            ..RustpotterConfig::default()
        };
        let mut d = Rustpotter::new(&cfg).expect("rustpotter");
        d.add_wakeword_from_file("hey_gaia", MODEL_PATH).expect("load model");
        let hit = hey.iter().filter(|p| detect(&mut d, &read_wav(p)).0 > 0).count();
        let nfp = neg.iter().filter(|p| detect(&mut d, &read_wav(p)).0 > 0).count();
        let bfp = bg.iter().filter(|p| detect(&mut d, &read_wav(p)).0 > 0).count();
        let per_hour = if bg_secs > 1.0 { format!("{:.1}", bfp as f32 / bg_secs * 3600.0) } else { "n/a".into() };
        println!("{thr:>9.2} | {hit:>8} {:>8} | {nfp:>8} | {per_hour:>12}", hey.len() - hit);
    }
    println!("(bg windows overlap, so a single noise event can count twice)");
}
