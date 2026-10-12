//! Streaming sample-rate conversion for the live microphone path.
//!
//! [`utterance::to_16k_i16`] converts a whole finished recording at once. The hands-free listener instead
//! receives the microphone in small chunks that never end, so this converter keeps its state between
//! chunks: how the output looks must not depend on where the chunk boundaries happened to fall.

use super::utterance::TARGET_RATE;

/// Mono f32 at any rate → mono f32 at 16 kHz, chunk by chunk.
///
/// A causal box filter (so higher rates do not alias) followed by linear interpolation, the same method as
/// the whole-recording converter, with its small fixed delay of half a filter window (about 0.5 ms).
pub struct Downsampler {
    /// Input samples per output sample.
    step: f64,
    window: Vec<f32>,
    next_slot: usize,
    filled: usize,
    sum: f64,
    prev: f32,
    /// Input index of the sample about to be consumed.
    index: u64,
    /// Position (in input samples) of the next output sample.
    phase: f64,
}

impl Downsampler {
    pub fn new(rate: u32) -> Self {
        let rate = rate.max(1);
        let width = (rate as f32 / TARGET_RATE as f32).round().max(1.0) as usize;
        Self {
            step: rate as f64 / TARGET_RATE as f64,
            window: vec![0.0; width],
            next_slot: 0,
            filled: 0,
            sum: 0.0,
            prev: 0.0,
            index: 0,
            phase: 0.0,
        }
    }

    /// Feeds one chunk and appends the 16 kHz samples it completes to `out`.
    pub fn push(&mut self, chunk: &[f32], out: &mut Vec<f32>) {
        let width = self.window.len();
        for &x in chunk {
            // boxcar over the last `width` samples
            self.sum += (x - self.window[self.next_slot]) as f64;
            self.window[self.next_slot] = x;
            self.next_slot = (self.next_slot + 1) % width;
            self.filled = (self.filled + 1).min(width);
            let filtered = (self.sum / self.filled as f64) as f32;

            while self.phase <= self.index as f64 {
                let frac = (self.phase - (self.index as f64 - 1.0)).clamp(0.0, 1.0) as f32;
                out.push(self.prev + (filtered - self.prev) * frac);
                self.phase += self.step;
            }
            self.prev = filtered;
            self.index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(rate: u32, secs: f32, hz: f32) -> Vec<f32> {
        (0..(rate as f32 * secs) as usize)
            .map(|i| 0.5 * (i as f32 * hz * std::f32::consts::TAU / rate as f32).sin())
            .collect()
    }

    fn run(rate: u32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut d = Downsampler::new(rate);
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            d.push(c, &mut out);
        }
        out
    }

    #[test]
    fn output_length_follows_the_rate_ratio() {
        for rate in [16_000u32, 44_100, 48_000] {
            let out = run(rate, &tone(rate, 1.0, 300.0), 480);
            assert!((out.len() as i32 - 16_000).abs() <= 3, "{rate}: {} samples", out.len());
        }
    }

    #[test]
    fn the_pitch_survives() {
        let out = run(48_000, &tone(48_000, 1.0, 400.0), 480);
        let crossings = out.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count();
        assert!((crossings as i32 - 800).abs() < 20, "{crossings} zero crossings");
    }

    #[test]
    fn chunk_boundaries_do_not_change_the_output() {
        let input = tone(44_100, 0.5, 250.0);
        let whole = run(44_100, &input, input.len());
        for chunk in [1usize, 7, 441, 480, 1000] {
            let pieces = run(44_100, &input, chunk);
            assert_eq!(pieces.len(), whole.len(), "chunk {chunk}");
            for (a, b) in pieces.iter().zip(&whole) {
                assert!((a - b).abs() < 1e-4, "chunk {chunk} differs");
            }
        }
    }

    #[test]
    fn a_constant_input_stays_constant() {
        let out = run(48_000, &vec![0.25f32; 4_800], 160);
        assert!(out.iter().skip(5).all(|v| (v - 0.25).abs() < 1e-3));
    }

    #[test]
    fn silence_in_is_silence_out_and_nothing_in_is_nothing_out() {
        assert!(run(48_000, &vec![0.0; 4_800], 480).iter().all(|v| *v == 0.0));
        let mut d = Downsampler::new(48_000);
        let mut out = Vec::new();
        d.push(&[], &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_zero_rate_does_not_panic() {
        let mut d = Downsampler::new(0);
        let mut out = Vec::new();
        d.push(&[0.1, 0.2], &mut out);
    }
}
