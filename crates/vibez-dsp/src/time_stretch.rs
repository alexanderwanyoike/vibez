//! Time stretch for audio clips.
//!
//! Two paths live side-by-side:
//!
//! - `linear_resample` / `stretch_mono` / `stretch_to` — linear-interp
//!   resampler. Pitch-shifts proportionally to the stretch ratio.
//!   Cheap, robust, and suited to audio quantize where per-slice
//!   ratios can be extreme (5× or more on sparse transients) and the
//!   pitch shift on drums is either inaudible or musically acceptable.
//!
//! - `pitch_preserving_stretch` / `wsola_stretch_mono` /
//!   `wsola_stretch_stereo` / `pitch_preserving_stretch_mono` —
//!   WSOLA-based stretcher for ratios in [0.5, 2.0]. Preserves pitch
//!   for bass and melodic material (the primary reason for having it).
//!   Falls back to `linear_resample` for ratios outside that band so
//!   the caller can reuse one entrypoint without branching.
//!
//! WSOLA (Waveform Similarity Overlap-Add, Verhelst & Roelands 1993)
//! hand-rolled in pure Rust. Frame 1024, synthesis hop 256 (75%
//! overlap), Hann window, ±256 sample search, normalised
//! cross-correlation on the first `hop` samples, per-sample gain
//! normalisation. Stereo shares the analysis on a mono mix so the
//! stereo image stays coherent (classic bug: independent per-channel
//! search collapses the image).

use vibez_core::audio_buffer::DecodedAudio;

// WSOLA parameters.
const WSOLA_FRAME: usize = 1024;
const WSOLA_HOP_SYN: usize = 256;
const WSOLA_SEARCH: isize = 256;

// Ratio router.
const RATIO_PASSTHROUGH_EPS: f64 = 0.005;
const RATIO_MIN: f64 = 0.5;
const RATIO_MAX: f64 = 2.0;

/// Stretch `audio` to the given target frame count using linear-interp
/// resampling (pitch-shifting). Sample rate is preserved.
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

/// Stretch a mono buffer to `target_len` samples via linear-interp
/// resampling (pitch-shifting).
pub fn stretch_mono(input: &[f32], target_len: usize) -> Vec<f32> {
    linear_resample(input, target_len)
}

/// Pitch-preserving stretch of a decoded buffer. Dispatches to the
/// stereo path when there are exactly two channels so that the stereo
/// image stays coherent under stretch.
pub fn pitch_preserving_stretch(audio: &DecodedAudio, target_frames: usize) -> DecodedAudio {
    let src_frames = audio.num_frames();
    if src_frames == 0 || target_frames == 0 {
        return DecodedAudio {
            channels: audio
                .channels
                .iter()
                .map(|_| vec![0.0; target_frames])
                .collect(),
            sample_rate: audio.sample_rate,
        };
    }
    let ratio = target_frames as f64 / src_frames as f64;

    if (ratio - 1.0).abs() < RATIO_PASSTHROUGH_EPS {
        return resize_clone(audio, target_frames);
    }

    if !(RATIO_MIN..=RATIO_MAX).contains(&ratio) || src_frames < WSOLA_FRAME * 2 {
        return stretch_to(audio, target_frames);
    }

    if audio.channels.len() == 2 {
        let (left, right) =
            wsola_stretch_stereo(&audio.channels[0], &audio.channels[1], target_frames);
        DecodedAudio {
            channels: vec![left, right],
            sample_rate: audio.sample_rate,
        }
    } else {
        let channels = audio
            .channels
            .iter()
            .map(|ch| pitch_preserving_stretch_mono(ch, target_frames))
            .collect();
        DecodedAudio {
            channels,
            sample_rate: audio.sample_rate,
        }
    }
}

/// Pitch-preserving stretch of a single channel. Falls back to the
/// linear resampler for extreme ratios or short inputs.
pub fn pitch_preserving_stretch_mono(input: &[f32], target_len: usize) -> Vec<f32> {
    if input.is_empty() || target_len == 0 {
        return vec![0.0; target_len];
    }
    let ratio = target_len as f64 / input.len() as f64;
    if (ratio - 1.0).abs() < RATIO_PASSTHROUGH_EPS {
        let mut out = input.to_vec();
        out.resize(target_len, 0.0);
        return out;
    }
    if !(RATIO_MIN..=RATIO_MAX).contains(&ratio) || input.len() < WSOLA_FRAME * 2 {
        return linear_resample(input, target_len);
    }
    let deltas = wsola_analyze(input, target_len);
    wsola_synthesize(input, target_len, &deltas)
}

/// Shorthand for the mono WSOLA path. Same ratio guards as
/// `pitch_preserving_stretch_mono`.
pub fn wsola_stretch_mono(input: &[f32], target_len: usize) -> Vec<f32> {
    pitch_preserving_stretch_mono(input, target_len)
}

/// Pitch-preserving stereo stretch. Runs the WSOLA analysis on the
/// mono sum so the same frame offsets are applied to both channels,
/// preserving inter-channel phase relationships.
pub fn wsola_stretch_stereo(
    left: &[f32],
    right: &[f32],
    target_len: usize,
) -> (Vec<f32>, Vec<f32>) {
    if left.is_empty() || right.is_empty() || target_len == 0 {
        return (vec![0.0; target_len], vec![0.0; target_len]);
    }
    let src = left.len().min(right.len());
    let ratio = target_len as f64 / src as f64;
    if (ratio - 1.0).abs() < RATIO_PASSTHROUGH_EPS {
        let mut l = left[..src].to_vec();
        let mut r = right[..src].to_vec();
        l.resize(target_len, 0.0);
        r.resize(target_len, 0.0);
        return (l, r);
    }
    if !(RATIO_MIN..=RATIO_MAX).contains(&ratio) || src < WSOLA_FRAME * 2 {
        return (
            linear_resample(left, target_len),
            linear_resample(right, target_len),
        );
    }
    let mono: Vec<f32> = (0..src).map(|i| 0.5 * (left[i] + right[i])).collect();
    let deltas = wsola_analyze(&mono, target_len);
    let l_out = wsola_synthesize(&left[..src], target_len, &deltas);
    let r_out = wsola_synthesize(&right[..src], target_len, &deltas);
    (l_out, r_out)
}

fn resize_clone(audio: &DecodedAudio, target_frames: usize) -> DecodedAudio {
    let channels = audio
        .channels
        .iter()
        .map(|ch| {
            let mut v = ch.clone();
            v.resize(target_frames, 0.0);
            v
        })
        .collect();
    DecodedAudio {
        channels,
        sample_rate: audio.sample_rate,
    }
}

/// Run the WSOLA analysis pass and return a sequence of per-frame
/// offsets (delta values) from the natural analysis positions.
fn wsola_analyze(input: &[f32], target_len: usize) -> Vec<isize> {
    let ratio = target_len as f64 / input.len() as f64;
    let ha_f = WSOLA_HOP_SYN as f64 / ratio;
    let pad = WSOLA_FRAME / 2;
    let padded = pad_zeros(input, pad);
    let n_padded = padded.len();
    let padded_out_len = target_len + pad * 2;

    let mut deltas = Vec::new();
    let mut prev_pos: isize = 0;

    let mut k: usize = 0;
    loop {
        let syn_pos = k * WSOLA_HOP_SYN;
        if syn_pos >= padded_out_len {
            break;
        }
        let base = (k as f64 * ha_f) as isize;
        let delta = if k == 0 {
            0
        } else {
            let target_start = prev_pos + WSOLA_HOP_SYN as isize;
            best_delta(&padded, target_start, base, WSOLA_SEARCH)
        };
        let pos = (base + delta).max(0);
        let pos_usize = pos as usize;
        if pos_usize + WSOLA_FRAME > n_padded {
            break;
        }
        deltas.push(delta);
        prev_pos = pos;
        k += 1;
    }
    deltas
}

/// Synthesise the stretched output using a precomputed delta sequence.
/// Each channel in a stereo pair goes through this with the same
/// deltas so inter-channel phase alignment is preserved.
fn wsola_synthesize(input: &[f32], target_len: usize, deltas: &[isize]) -> Vec<f32> {
    let ratio = target_len as f64 / input.len() as f64;
    let ha_f = WSOLA_HOP_SYN as f64 / ratio;
    let window = hann(WSOLA_FRAME);
    let pad = WSOLA_FRAME / 2;
    let padded = pad_zeros(input, pad);
    let n_padded = padded.len();
    let padded_out_len = target_len + pad * 2;

    let mut out_sum = vec![0.0f32; padded_out_len];
    let mut out_win = vec![0.0f32; padded_out_len];

    for (k, &delta) in deltas.iter().enumerate() {
        let syn_pos = k * WSOLA_HOP_SYN;
        if syn_pos >= padded_out_len {
            break;
        }
        let base = (k as f64 * ha_f) as isize;
        let pos = (base + delta).max(0);
        let pos_usize = pos as usize;
        if pos_usize + WSOLA_FRAME > n_padded {
            break;
        }
        for j in 0..WSOLA_FRAME {
            let out_idx = syn_pos + j;
            if out_idx >= padded_out_len {
                break;
            }
            let w = window[j];
            out_sum[out_idx] += w * padded[pos_usize + j];
            out_win[out_idx] += w;
        }
    }

    let mut out = vec![0.0f32; target_len];
    for (i, out_sample) in out.iter_mut().enumerate() {
        let src_idx = i + pad;
        let w = out_win[src_idx];
        *out_sample = if w > 1e-6 { out_sum[src_idx] / w } else { 0.0 };
    }
    out
}

fn best_delta(input: &[f32], target_start: isize, base: isize, radius: isize) -> isize {
    let n = input.len() as isize;
    let hop = WSOLA_HOP_SYN as isize;
    if target_start < 0 || target_start + hop > n {
        return 0;
    }
    let target = &input[target_start as usize..(target_start as usize + WSOLA_HOP_SYN)];
    let mut t_sq = 0.0f32;
    for &t in target {
        t_sq += t * t;
    }
    if t_sq < 1e-10 {
        return 0;
    }
    let t_norm = t_sq.sqrt();

    let mut best = 0isize;
    let mut best_score = f32::NEG_INFINITY;
    for d in -radius..=radius {
        let start = base + d;
        if start < 0 || start + hop > n {
            continue;
        }
        let cand = &input[start as usize..(start as usize + WSOLA_HOP_SYN)];
        let mut dot = 0.0f32;
        let mut c_sq = 0.0f32;
        for i in 0..WSOLA_HOP_SYN {
            dot += cand[i] * target[i];
            c_sq += cand[i] * cand[i];
        }
        if c_sq < 1e-10 {
            continue;
        }
        let score = dot / (c_sq.sqrt() * t_norm);
        if score > best_score {
            best_score = score;
            best = d;
        }
    }
    best
}

fn hann(n: usize) -> Vec<f32> {
    if n <= 1 {
        return vec![0.0; n];
    }
    (0..n)
        .map(|i| {
            let x = 2.0 * std::f32::consts::PI * i as f32 / (n - 1) as f32;
            0.5 * (1.0 - x.cos())
        })
        .collect()
}

fn pad_zeros(input: &[f32], pad: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; input.len() + pad * 2];
    out[pad..pad + input.len()].copy_from_slice(input);
    out
}

/// Linear-interp resampler. Produces exactly `target_len` samples.
pub fn linear_resample(input: &[f32], target_len: usize) -> Vec<f32> {
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

    fn rms(v: &[f32]) -> f32 {
        let n = v.len().max(1) as f32;
        (v.iter().map(|s| s * s).sum::<f32>() / n).sqrt()
    }

    /// Brute-force ACF pitch estimator. Searches lags in
    /// `[sr/freq_max, sr/freq_min]` and returns the frequency
    /// corresponding to the dominant ACF peak.
    fn dominant_frequency(signal: &[f32], sr: u32, freq_min: f32, freq_max: f32) -> f32 {
        let min_lag = (sr as f32 / freq_max).max(1.0) as usize;
        let max_lag = ((sr as f32 / freq_min) as usize).min(signal.len().saturating_sub(1));
        if max_lag <= min_lag {
            return 0.0;
        }
        let n = signal.len();
        let mut best_lag = min_lag;
        let mut best_val = f64::NEG_INFINITY;
        for lag in min_lag..=max_lag {
            let mut sum = 0.0f64;
            for i in 0..n - lag {
                sum += (signal[i] as f64) * (signal[i + lag] as f64);
            }
            let val = sum / (n - lag) as f64;
            if val > best_val {
                best_val = val;
                best_lag = lag;
            }
        }
        sr as f32 / best_lag as f32
    }

    fn stereo_correlation(l: &[f32], r: &[f32]) -> f32 {
        let n = l.len().min(r.len()).max(1);
        let mut dot = 0.0f64;
        let mut ll = 0.0f64;
        let mut rr = 0.0f64;
        for i in 0..n {
            dot += (l[i] as f64) * (r[i] as f64);
            ll += (l[i] as f64).powi(2);
            rr += (r[i] as f64).powi(2);
        }
        let denom = (ll * rr).sqrt().max(1e-12);
        (dot / denom) as f32
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

    #[test]
    fn wsola_identity_when_ratio_is_one() {
        let input = sine(440.0, 44_100, 16_384);
        let out = pitch_preserving_stretch_mono(&input, input.len());
        assert_eq!(out.len(), input.len());
    }

    #[test]
    fn wsola_produces_exact_output_length_stretch() {
        let input = sine(440.0, 44_100, 16_384);
        let out = pitch_preserving_stretch_mono(&input, 19_169);
        assert_eq!(out.len(), 19_169);
    }

    #[test]
    fn wsola_produces_exact_output_length_compress() {
        let input = sine(440.0, 44_100, 16_384);
        let out = pitch_preserving_stretch_mono(&input, 13_926);
        assert_eq!(out.len(), 13_926);
    }

    #[test]
    fn wsola_preserves_pitch_440hz_stretched() {
        let sr = 44_100u32;
        let input = sine(440.0, sr, 32_768);
        let target_len = (input.len() as f64 * 1.17) as usize;
        let out = pitch_preserving_stretch_mono(&input, target_len);
        let freq = dominant_frequency(&out, sr, 380.0, 500.0);
        assert!(
            (freq - 440.0).abs() < 5.0,
            "pitch drifted: expected ~440, got {freq}"
        );
    }

    #[test]
    fn wsola_preserves_pitch_440hz_compressed() {
        let sr = 44_100u32;
        let input = sine(440.0, sr, 32_768);
        let target_len = (input.len() as f64 * 0.85) as usize;
        let out = pitch_preserving_stretch_mono(&input, target_len);
        let freq = dominant_frequency(&out, sr, 380.0, 500.0);
        assert!(
            (freq - 440.0).abs() < 5.0,
            "pitch drifted: expected ~440, got {freq}"
        );
    }

    #[test]
    fn wsola_preserves_pitch_220hz() {
        let sr = 44_100u32;
        let input = sine(220.0, sr, 32_768);
        let target_len = (input.len() as f64 * 1.17) as usize;
        let out = pitch_preserving_stretch_mono(&input, target_len);
        let freq = dominant_frequency(&out, sr, 180.0, 280.0);
        assert!(
            (freq - 220.0).abs() < 3.0,
            "pitch drifted: expected ~220, got {freq}"
        );
    }

    #[test]
    fn wsola_preserves_pitch_110hz() {
        let sr = 44_100u32;
        let input = sine(110.0, sr, 65_536);
        let target_len = (input.len() as f64 * 0.85) as usize;
        let out = pitch_preserving_stretch_mono(&input, target_len);
        let freq = dominant_frequency(&out, sr, 90.0, 140.0);
        assert!(
            (freq - 110.0).abs() < 2.0,
            "pitch drifted: expected ~110, got {freq}"
        );
    }

    #[test]
    fn linear_stretch_shifts_pitch_for_comparison() {
        // Contrast check: confirm the linear path is the one that
        // detunes. If this ever fails, WSOLA is no longer the only
        // pitch-preserving path.
        let sr = 44_100u32;
        let input = sine(440.0, sr, 32_768);
        let out = linear_resample(&input, (input.len() as f64 * 0.85) as usize);
        let freq = dominant_frequency(&out, sr, 400.0, 600.0);
        assert!(
            (freq - 440.0 / 0.85).abs() < 20.0,
            "linear stretch should detune to ~517.6 Hz, got {freq}"
        );
    }

    #[test]
    fn wsola_rms_envelope_is_smooth() {
        let sr = 44_100u32;
        let input = sine(220.0, sr, 32_768);
        let target_len = (input.len() as f64 * 1.17) as usize;
        let out = pitch_preserving_stretch_mono(&input, target_len);

        // Skip the first few hops (startup transient with partial overlap
        // coverage near the padded edge) and the last few (tail).
        let win = 1_024usize;
        let skip = WSOLA_HOP_SYN * 4;
        let mut rms_values = Vec::new();
        let mut i = skip;
        while i + win < out.len().saturating_sub(skip) {
            rms_values.push(rms(&out[i..i + win]));
            i += win / 2;
        }
        assert!(rms_values.len() > 4);
        let max = rms_values.iter().cloned().fold(0.0_f32, f32::max);
        let min = rms_values.iter().cloned().fold(f32::INFINITY, f32::min);
        // 0.3 dB ≈ ratio 1.035. WSOLA with per-sample gain
        // normalisation should sit well inside that.
        let ratio_db = 20.0 * (max / min.max(1e-6)).log10();
        assert!(
            ratio_db < 0.5,
            "RMS envelope ripple too large: {ratio_db} dB (min {min} max {max})"
        );
    }

    #[test]
    fn wsola_stereo_preserves_phase_coherence() {
        let sr = 44_100u32;
        let left = sine(220.0, sr, 32_768);
        let right = left.clone(); // perfectly correlated
        let target = (left.len() as f64 * 1.17) as usize;
        let (lo, ro) = wsola_stretch_stereo(&left, &right, target);
        assert_eq!(lo.len(), target);
        assert_eq!(ro.len(), target);
        let corr = stereo_correlation(&lo, &ro);
        assert!(
            corr > 0.98,
            "stereo coherence lost after stretch: correlation {corr}"
        );
    }

    #[test]
    fn pitch_preserving_stretch_dispatches_stereo() {
        let sr = 44_100u32;
        let audio = DecodedAudio {
            channels: vec![sine(220.0, sr, 32_768), sine(220.0, sr, 32_768)],
            sample_rate: sr,
        };
        let out = pitch_preserving_stretch(&audio, (audio.num_frames() as f64 * 1.17) as usize);
        assert_eq!(out.num_channels(), 2);
        let corr = stereo_correlation(&out.channels[0], &out.channels[1]);
        assert!(corr > 0.98);
    }

    #[test]
    fn pitch_preserving_stretch_falls_back_for_extreme_ratios() {
        // 4× stretch is outside the WSOLA band; expected to fall back
        // to the linear path without panicking, and produce the right
        // length.
        let input = sine(440.0, 44_100, 2_048);
        let out = pitch_preserving_stretch_mono(&input, 8_192);
        assert_eq!(out.len(), 8_192);
    }
}
