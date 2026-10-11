//! Phase 0 spike (throwaway). Usage:
//!   voice-phase0 devices              list input/output devices with stable ids
//!   voice-phase0 meter [secs] [id]    capture, print level only — nothing is written to disk
mod aec_sim;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, SampleFormat};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("meter") => meter(
            args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5),
            args.get(3).cloned(),
        ),
        Some("aec-sim") => {
            for (d, g, dt) in [(30, 0.5, false), (30, 0.5, true), (120, 0.8, true)] {
                aec_sim::run(d, g, dt);
            }
        }
        _ => devices(),
    }
}

fn devices() {
    let host = cpal::default_host();
    println!("host: {}", host.id());
    let def_in = host.default_input_device().and_then(|d| d.id().ok());
    let def_out = host.default_output_device().and_then(|d| d.id().ok());
    println!("-- inputs");
    for d in host.input_devices().expect("input_devices") {
        let id = d.id().map(|i| i.to_string()).unwrap_or_default();
        let name = d.description().map(|x| x.name().to_string()).unwrap_or_default();
        let cfg = d.default_input_config().map(|c| format!("{c:?}")).unwrap_or_else(|e| e.to_string());
        let star = if def_in.as_ref().map(|i| i.to_string()) == Some(id.clone()) { "*" } else { " " };
        println!("{star} {name}\n    id={id}\n    {cfg}");
    }
    println!("-- outputs");
    for d in host.output_devices().expect("output_devices") {
        let id = d.id().map(|i| i.to_string()).unwrap_or_default();
        let name = d.description().map(|x| x.name().to_string()).unwrap_or_default();
        let star = if def_out.as_ref().map(|i| i.to_string()) == Some(id.clone()) { "*" } else { " " };
        println!("{star} {name}\n    id={id}");
    }
}

fn meter(secs: u64, id: Option<String>) {
    let host = cpal::default_host();
    let device = match id {
        Some(s) => host.device_by_id(&s.parse().expect("bad id")).expect("no such device"),
        None => host.default_input_device().expect("no default input"),
    };
    let cfg = device.default_input_config().expect("config");
    println!("capturing from {:?}: {cfg:?}", device.description().map(|d| d.name().to_string()));
    let peak = Arc::new(AtomicU64::new(0));
    let frames = Arc::new(AtomicU64::new(0));
    let (p, f) = (peak.clone(), frames.clone());
    let err_fn = |e: cpal::Error| match e.kind() {
        ErrorKind::DeviceChanged => eprintln!("DEVICE CHANGED: {e}"),
        _ => eprintln!("stream error: {e}"),
    };
    let ch = cfg.channels() as u64;
    let stream = match cfg.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            cfg.into(),
            move |d: &[f32], _: &_| {
                f.fetch_add(d.len() as u64 / ch, Ordering::Relaxed);
                let m = d.iter().fold(0f32, |a, s| a.max(s.abs()));
                p.fetch_max((m * 1_000_000.0) as u64, Ordering::Relaxed);
            },
            err_fn,
            None,
        ),
        other => panic!("unhandled sample format {other:?}"),
    }
    .expect("build stream");
    let t0 = Instant::now();
    stream.play().expect("play");
    let rate = frames_rate(&cfg_rate(&device));
    for _ in 0..secs {
        std::thread::sleep(Duration::from_secs(1));
        println!("peak={:.4}  frames={}", peak.swap(0, Ordering::Relaxed) as f32 / 1e6, frames.load(Ordering::Relaxed));
    }
    let d0 = Instant::now();
    drop(stream);
    println!("ran {:?}, drop took {:?} (rate {rate})", t0.elapsed(), d0.elapsed());
}

fn cfg_rate(d: &cpal::Device) -> u32 { d.default_input_config().map(|c| c.sample_rate()).unwrap_or(0) }
fn frames_rate(r: &u32) -> u32 { *r }
