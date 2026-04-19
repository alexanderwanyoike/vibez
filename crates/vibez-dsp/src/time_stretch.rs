//! Time stretch (pitch-shifting) via linear-interpolation resampling.
//!
//! Used by audio quantize to fit each transient slice into its
//! grid-aligned target length. Pitch shifts proportionally to the
//! stretch ratio, which is usually what you want for drum loops
//! (pitches kicks up when compressing, down when expanding). Output
//! length is exactly `target_len`.
//!
//! A pitch-preserving variant (WSOLA / phase vocoder) is tracked as
//! a follow-up for melodic content; for drum-first electronic work
//! the pitch-shift side effect is typically either inaudible (small
//! ratios) or musically useful (larger ratios).

use vibez_core::audio_buffer::DecodedAudio;

/// Stretch `audio` to the given target frame count. Per-channel output
/// length is exactly `target_frames`. Sample rate is preserved.
pub fn stretch_to(audio: &DecodedAudio, target_frames: usize) -> DecodedAudio {
    let channels = audio
        .channels
        .iter()
        .map(|ch| stretch_mono(ch, target_frames))
        .collect();
    DecodedAudio {
        channels,
        sample_rate: audio.sample_rate,
    }
}

/// Stretch a mono buffer to `target_len` samples.
pub fn stretch_mono(input: &[f32], target_len: usize) -> Vec<f32> {
    linear_resample(input, target_len)
}

/// Linear-interp resampler. Produces exactly `target_len` samples.
fn linear_resample(input: &[f32], target_len: usize) -> Vec<f32> {
    if input.is_empty() {
        return vec![0.0; target_len];
    }
    if target_len == 0 {
        return Vec::new();
    }
    let last_idx = (input.len() - 1) as f64;
    (0..target_len)
        .map(|i| {
            let t = if target_len > 1 {
                i as f64 / (target_len - 1) as f64
            } else {
                0.0
            };
            let src_pos = t * last_idx;
            let lo = src_pos.floor() as usize;
            let hi = (lo + 1).min(input.len() - 1);
            let frac = (src_pos - lo as f64) as f32;
            input[lo] * (1.0 - frac) + input[hi] * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f32, sr: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn identity_when_target_equals_source() {
        let input = sine(440.0, 44_100, 4_096);
        let out = stretch_mono(&input, input.len());
        assert_eq!(out.len(), input.len());
        for (a, b) in out.iter().zip(input.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn stretch_produces_exact_output_length() {
        let input = sine(220.0, 44_100, 8_192);
        let out = stretch_mono(&input, 12_288);
        assert_eq!(out.len(), 12_288);
    }

    #[test]
    fn compress_produces_exact_output_length() {
        let input = sine(220.0, 44_100, 8_192);
        let out = stretch_mono(&input, 5_000);
        assert_eq!(out.len(), 5_000);
    }

    #[test]
    fn short_input_falls_back_without_panicking() {
        let input = sine(440.0, 44_100, 200);
        let out = stretch_mono(&input, 400);
        assert_eq!(out.len(), 400);
        let peak = out.iter().map(|s| s.abs()).fold(0.0_f32, f32::max);
        assert!(peak > 0.0);
    }

    #[test]
    fn stretched_output_preserves_energy_order_of_magnitude() {
        // For a sine, RMS should be roughly preserved (pitch preserving).
        let input = sine(440.0, 44_100, 16_384);
        let in_rms = rms(&input);
        let out = stretch_mono(&input, 24_576);
        let out_rms = rms(&out);
        assert!(
            (out_rms / in_rms - 1.0).abs() < 0.3,
            "in_rms {in_rms} out_rms {out_rms}"
        );
    }

    #[test]
    fn stretch_to_preserves_channel_count() {
        let audio = DecodedAudio {
            channels: vec![sine(440.0, 44_100, 4_096), sine(440.0, 44_100, 4_096)],
            sample_rate: 44_100,
        };
        let out = stretch_to(&audio, 6_144);
        assert_eq!(out.num_channels(), 2);
        assert_eq!(out.num_frames(), 6_144);
    }

    fn rms(v: &[f32]) -> f32 {
        let n = v.len().max(1) as f32;
        (v.iter().map(|s| s * s).sum::<f32>() / n).sqrt()
    }
}
