//! Speech-to-text check (Phase 0 spike, throwaway): does Mistral Voxtral still understand the user while they talk
//! over Gaia, after echo cancellation?
//!
//!   MISTRAL_API_KEY=... voice-phase0 stt-test [dir ...] [--lang nl]
//!
//! For each saved echo run (recordings/echo/**), cuts the 12-19 s window where the user talks over Gaia and
//! transcribes it raw (the mic as recorded) and after several echo-cancellation variants. The key is read only
//! from the environment and never printed or stored. Audio leaves the machine: it is sent to api.mistral.ai.
use crate::replay;
use std::io::Cursor;
use std::path::{Path, PathBuf};

const SR: usize = 16_000;
const MODEL: &str = "voxtral-mini-latest";

fn read(path: &Path) -> Vec<f32> {
    hound::WavReader::open(path).expect("open wav").samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect()
}

fn wav_bytes(x: &[f32]) -> Vec<u8> {
    let spec = hound::WavSpec { channels: 1, sample_rate: SR as u32, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut cur = Cursor::new(Vec::new());
    {
        let mut w = hound::WavWriter::new(&mut cur, spec).unwrap();
        for s in x {
            w.write_sample((s.clamp(-1.0, 1.0) * 32767.0) as i16).unwrap();
        }
        w.finalize().unwrap();
    }
    cur.into_inner()
}

fn transcribe(key: &str, audio: &[f32], lang: Option<&str>) -> String {
    // normalise to a sensible level so a quiet signal is not penalised just for being quiet
    let peak = audio.iter().fold(0f32, |a, s| a.max(s.abs())).max(1e-6);
    let norm: Vec<f32> = audio.iter().map(|s| s * (0.7 / peak)).collect();
    let part = reqwest::blocking::multipart::Part::bytes(wav_bytes(&norm)).file_name("clip.wav").mime_str("audio/wav").unwrap();
    let mut form = reqwest::blocking::multipart::Form::new().text("model", MODEL).part("file", part);
    if let Some(l) = lang {
        form = form.text("language", l.to_string());
    }
    let resp = reqwest::blocking::Client::new()
        .post("https://api.mistral.ai/v1/audio/transcriptions")
        .bearer_auth(key)
        .multipart(form)
        .send();
    match resp {
        Ok(r) => {
            let status = r.status();
            let body: serde_json::Value = r.json().unwrap_or(serde_json::Value::Null);
            if status.is_success() {
                body["text"].as_str().unwrap_or("").trim().to_string()
            } else {
                format!("[HTTP {status}] {}", body["message"].as_str().or(body["detail"].as_str()).unwrap_or("request failed"))
            }
        }
        Err(e) => format!("[request failed: {e}]"),
    }
}

pub fn run(dirs: Vec<String>, lang: Option<String>) {
    let key = match std::env::var("MISTRAL_API_KEY") {
        Ok(k) if !k.trim().is_empty() => k,
        _ => {
            eprintln!("set MISTRAL_API_KEY in this shell first (it is read from the environment only)");
            return;
        }
    };
    let mut targets: Vec<PathBuf> = dirs.into_iter().map(PathBuf::from).collect();
    if targets.is_empty() {
        let root = Path::new("recordings/echo");
        targets.push(root.to_path_buf());
        if let Ok(rd) = std::fs::read_dir(root) {
            targets.extend(rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()));
        }
    }
    let win = |x: &[f32]| x[(12 * SR).min(x.len())..(19 * SR).min(x.len())].to_vec();
    for d in targets {
        if !(d.join("reference.wav").exists() && d.join("mic_aligned.wav").exists()) {
            continue;
        }
        let (r, m) = (read(&d.join("reference.wav")), read(&d.join("mic_aligned.wav")));
        let n = r.len().min(m.len());
        let (r, m) = (&r[..n], &m[..n]);
        println!("\n== {} ==", d.display());
        let variants: Vec<(&str, Vec<f32>)> = vec![
            ("raw mic", m.to_vec()),
            ("sonora default", replay::sonora_pass(r, m)),
            ("aec3 default", replay::aec3_run(r, m, None)),
            ("aec3 relaxed x4", replay::aec3_run(r, m, Some(replay::gentle(2)))),
        ];
        for (name, out) in variants {
            let text = transcribe(&key, &win(&out), lang.as_deref());
            println!("  {name:<16} -> {text}");
        }
    }
}
