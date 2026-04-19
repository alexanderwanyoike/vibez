//! Pitch-preserving time stretch via WSOLA (Waveform Similarity
//! Overlap-Add).
//!
//! Used by audio quantize to fit each transient slice into its
//! grid-aligned target length. Quality is good for drums / percussive
//! material and acceptable for most loops; not audiophile-grade for
//! solo melodic content, which is fine for a sample-first DAW.
//!
//! The algorithm keeps output pitch identical to the input and changes
//! duration by overlap-adding Hann-windowed frames picked from the
//! input at positions that maximise waveform similarity with the
//! previously-emitted frame.

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
    if input.is_empty() || target_len == 0 {
        return vec![0.0; target_len];
    }
    if input.len() == target_len {
        return input.to_vec();
    }

    // Small inputs / targets: skip WSOLA, use linear interpolation.
    // WSOLA needs at least one full frame of headroom.
    const FRAME: usize = 1024;
    const HOP: usize = 256; // 75% overlap
    const SEARCH: i32 = 256;

    if input.len() < FRAME * 2 || target_len < FRAME * 2 {
        return linear_resample(input, target_len);
    }

    let hann = hann_window(FRAME);
    let ratio = (input.len() - FRAME) as f64 / (target_len - FRAME) as f64;

    // Accumulators: output samples + per-sample window sum for normalisation.
    let mut acc_sum = vec![0.0f32; target_len];
    let mut acc_win = vec![0.0f32; target_len];

    // Place the first frame without a search.
    let mut prev_end_half = vec![0.0f32; HOP];
    {
        let start = 0usize;
        overlap_add(&input[start..start + FRAME], &hann, 0, &mut acc_sum, &mut acc_win);
        for (i, s) in input[start + FRAME - HOP..start + FRAME].iter().enumerate() {
            prev_end_half[i] = *s * hann[FRAME - HOP + i];
        }
    }

    let mut out_pos = HOP;
    while out_pos + FRAME <= target_len {
        let ideal = (out_pos as f64 * ratio) as i64;
        let lo = (ideal - SEARCH as i64).max(0) as usize;
        let hi = (ideal + SEARCH as i64)
            .min((input.len() - FRAME) as i64)
            .max(0) as usize;

        // Cross-correlate `prev_end_half` against the first HOP samples
        // of each candidate frame in [lo, hi].
        let mut best = ideal.max(0).min((input.len() - FRAME) as i64) as usize;
        let mut best_corr = f32::NEG_INFINITY;
        let mut cand = lo;
        while cand <= hi {
            let corr = correlate(&prev_end_half, &input[cand..cand + HOP]);
            if corr > best_corr {
                best_corr = corr;
                best = cand;
            }
            cand += 4; // coarse step; good enough for transient-heavy material
        }

        overlap_add(
            &input[best..best + FRAME],
            &hann,
            out_pos,
            &mut acc_sum,
            &mut acc_win,
        );

        // Update rolling "previous end half" from what we just placed.
        for i in 0..HOP {
            prev_end_half[i] = input[best + FRAME - HOP + i] * hann[FRAME - HOP + i];
        }
        out_pos += HOP;
    }

    // Normalise by the accumulated window sum to keep constant gain.
    let mut out = vec![0.0f32; target_len];
    for i in 0..target_len {
        if acc_win[i] > 1e-6 {
            out[i] = acc_sum[i] / acc_win[i];
        }
    }
    out
}

fn overlap_add(
    frame: &[f32],
    window: &[f32],
    dest_pos: usize,
    acc_sum: &mut [f32],
    acc_win: &mut [f32],
) {
    let n = frame.len().min(window.len());
    let end = (dest_pos + n).min(acc_sum.len());
    let len = end - dest_pos;
    for i in 0..len {
        let w = window[i];
        acc_sum[dest_pos + i] += frame[i] * w;
        acc_win[dest_pos + i] += w;
    }
}

fn correlate(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut sum = 0.0f32;
    for i in 0..n {
        sum += a[i] * b[i];
    }
    sum
}

fn hann_window(n: usize) -> Vec<f32> {
    (0..n)
        .map(|i| {
            0.5 - 0.5
                * (2.0 * std::f32::consts::PI * i as f32 / (n as f32 - 1.0)).cos()
        })
        .collect()
}

/// Tiny linear-interp resampler used as a fallback for very short
/// buffers where WSOLA's minimum frame size doesn't fit.
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
