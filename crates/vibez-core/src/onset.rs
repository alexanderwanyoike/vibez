//! Offline transient / onset detection and BPM estimation for audio
//! clips.
//!
//! Pure time-domain detector suited for drum and percussion material:
//! high-pass pre-emphasis, full-wave rectification, one-pole envelope,
//! positive-flux onset-detection function, adaptive threshold, and
//! peak picking with a refractory window. No external deps.
//!
//! Two entrypoints:
//! - `detect_onsets` returns `Vec<u64>` of absolute frame indices.
//!   Drives slice-and-snap audio quantize.
//! - `detect_bpm` returns an `Option<BpmEstimate>`. Autocorrelates the
//!   onset flux (so it works on sustained melodic material, not just
//!   percussion), octave-folds into [60, 180] BPM using Parncutt's
//!   preference curve, and gates on confidence so sparse or silent
//!   clips return `None` rather than guessing.

use crate::audio_buffer::DecodedAudio;

/// Minimum gap between successive onsets. Prevents multi-triggering
/// on a single transient.
const REFRACTORY_MS: f32 = 30.0;

/// Lookback used for the adaptive threshold's mean + std estimate.
const THRESHOLD_WINDOW_MS: f32 = 100.0;

/// Sample-count used to measure envelope-level change (onset function).
const FLUX_WINDOW: usize = 64;

/// Envelope follower time constants.
const ATTACK_MS: f32 = 5.0;
const RELEASE_MS: f32 = 50.0;

/// High-pass pre-emphasis coefficient.
const PREEMPHASIS: f32 = 0.97;

/// Detect onsets in `audio` and return sample indices.
///
/// `sensitivity` scales the adaptive threshold: `threshold = mean +
/// sensitivity * std`. Typical range is `1.0` (loose) to `2.5`
/// (tight). `1.5` is a reasonable default for drum loops.
pub fn detect_onsets(audio: &DecodedAudio, sensitivity: f32) -> Vec<u64> {
    let frames = audio.num_frames();
    if frames < FLUX_WINDOW * 2 {
        return Vec::new();
    }
    let sr = audio.sample_rate as f32;
    if sr <= 0.0 {
        return Vec::new();
    }

    let mono = mix_to_mono(audio, frames);
    let hp = high_pass_rectify(&mono);
    let env = envelope(&hp, sr);
    let odf = onset_flux(&env);

    let refractory = ((REFRACTORY_MS * 0.001) * sr) as usize;
    let threshold_window = ((THRESHOLD_WINDOW_MS * 0.001) * sr) as usize;
    let sensitivity = sensitivity.clamp(0.25, 5.0);

    // O(N) adaptive threshold via running prefix sums. The old O(N * W)
    // path turned into a multi-second hang on long clips.
    let mut prefix_sum = vec![0.0f64; odf.len() + 1];
    let mut prefix_sq = vec![0.0f64; odf.len() + 1];
    for i in 0..odf.len() {
        let v = odf[i] as f64;
        prefix_sum[i + 1] = prefix_sum[i] + v;
        prefix_sq[i + 1] = prefix_sq[i] + v * v;
    }

    let mut onsets = Vec::new();
    let mut last_peak: Option<usize> = None;

    for i in 1..frames.saturating_sub(1) {
        let window_start = i.saturating_sub(threshold_window);
        let window_end = i;
        let n = (window_end - window_start) as f64;
        if n < 1.0 {
            continue;
        }
        let sum = prefix_sum[window_end] - prefix_sum[window_start];
        let sum_sq = prefix_sq[window_end] - prefix_sq[window_start];
        let mean = sum / n;
        let variance = (sum_sq / n - mean * mean).max(0.0);
        let std = variance.sqrt() as f32;
        let threshold = mean as f32 + sensitivity * std.max(1e-6);

        let current = odf[i];
        let prev = odf[i - 1];
        let next = odf[i + 1];
        let is_peak = current > threshold && current >= prev && current >= next;
        if !is_peak {
            continue;
        }
        if let Some(last) = last_peak {
            if i - last < refractory {
                continue;
            }
        }
        onsets.push(i as u64);
        last_peak = Some(i);
    }
    onsets
}

fn mix_to_mono(audio: &DecodedAudio, frames: usize) -> Vec<f32> {
    let channels = audio.channels.len().max(1);
    let mut mono = Vec::with_capacity(frames);
    for i in 0..frames {
        let mut sum = 0.0f32;
        for ch in &audio.channels {
            sum += ch.get(i).copied().unwrap_or(0.0);
        }
        mono.push(sum / channels as f32);
    }
    mono
}

fn high_pass_rectify(mono: &[f32]) -> Vec<f32> {
    let mut out = Vec::with_capacity(mono.len());
    let mut prev = 0.0f32;
    for &x in mono {
        let y = x - PREEMPHASIS * prev;
        prev = x;
        out.push(y.abs());
    }
    out
}

fn envelope(hp: &[f32], sample_rate: f32) -> Vec<f32> {
    let mut env = Vec::with_capacity(hp.len());
    let a_coef = time_coef(ATTACK_MS, sample_rate);
    let r_coef = time_coef(RELEASE_MS, sample_rate);
    let mut e = 0.0f32;
    for &x in hp {
        let coef = if x > e { a_coef } else { r_coef };
        e = x + coef * (e - x);
        env.push(e);
    }
    env
}

fn time_coef(ms: f32, sr: f32) -> f32 {
    let t = (ms * 0.001).max(1e-5);
    (-1.0 / (t * sr)).exp()
}

fn onset_flux(env: &[f32]) -> Vec<f32> {
    let mut odf = vec![0.0f32; env.len()];
    for i in FLUX_WINDOW..env.len() {
        let diff = env[i] - env[i - FLUX_WINDOW];
        odf[i] = diff.max(0.0);
    }
    odf
}

/// Tempo estimate for a decoded audio buffer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BpmEstimate {
    pub bpm: f64,
    /// Relative strength of the best ACF peak vs. the runner-up,
    /// mapped into [0, 1]. Higher is more trustworthy.
    pub confidence: f32,
}

// BPM search range.
const BPM_MIN: f64 = 40.0;
const BPM_MAX: f64 = 280.0;
// Target octave for folded output (dance-music conventional range).
const BPM_FOLD_LO: f64 = 60.0;
const BPM_FOLD_HI: f64 = 180.0;
const BPM_FOLD_PREF: f64 = 120.0;
// Working sample rate for the autocorrelation. 500 Hz gives a lag
// precision of ~0.5 BPM at 120 BPM, further refined by parabolic
// interpolation around the ACF peak.
const ODF_DOWNSAMPLE_HZ: u32 = 500;
// Minimum clip length to attempt detection, in samples at a
// reference 44.1 kHz. Scaled per-call by the actual sample rate.
// 44_100 samples ≈ 1.0 s, which covers single-bar dance-music loops
// down to 120 BPM (a 1-bar loop at 120 BPM is exactly 2 s; at
// 140 BPM it is ~1.71 s; at 174 BPM it is ~1.38 s). Using 2 s as
// the previous threshold rejected almost every 1-bar drum loop the
// user is likely to drop in, which is why the detector was
// returning `None` so often.
const MIN_SECONDS_FOR_BPM_F64: f64 = 1.0;

/// Detect the tempo of `audio`. Returns `None` when the audio is too
/// short, too sparse, or the autocorrelation is not strong enough to
/// commit to a tempo. Output BPM is always in `[60, 180]` after
/// Parncutt octave folding.
pub fn detect_bpm(audio: &DecodedAudio, sample_rate: u32) -> Option<BpmEstimate> {
    let frames = audio.num_frames();
    if sample_rate == 0 {
        return None;
    }
    let min_frames = (sample_rate as f64 * MIN_SECONDS_FOR_BPM_F64) as usize;
    if frames < min_frames {
        return None;
    }
    let sr = sample_rate as f32;

    // Reuse the onset-detection building blocks for the flux signal.
    let mono = mix_to_mono(audio, frames);
    let hp = high_pass_rectify(&mono);
    let env = envelope(&hp, sr);
    let odf = onset_flux(&env);

    // Block-max downsample the ODF to a slower rate so the ACF stays
    // affordable and resolution in BPM space is predictable.
    let block = (sample_rate / ODF_DOWNSAMPLE_HZ).max(1) as usize;
    let ds: Vec<f32> = odf
        .chunks(block)
        .map(|c| c.iter().cloned().fold(0.0f32, f32::max))
        .collect();
    let odf_sr = sample_rate as f64 / block as f64;
    if ds.len() < 64 {
        return None;
    }

    // Remove DC so the ACF measures pattern similarity, not mean level.
    let mean: f64 = ds.iter().map(|&x| x as f64).sum::<f64>() / ds.len() as f64;
    let ds_dc: Vec<f32> = ds.iter().map(|&x| x - mean as f32).collect();

    let min_lag = (60.0 * odf_sr / BPM_MAX).max(2.0) as usize;
    let max_lag = ((60.0 * odf_sr / BPM_MIN) as usize).min(ds_dc.len().saturating_sub(1) / 2);
    if max_lag <= min_lag + 2 {
        return None;
    }

    // Biased ACF (constant-N divisor) in the lag range of interest.
    // Biased form naturally decays with lag, so peaks at the beat
    // period dominate over peaks at the 2-bar / 4-bar subharmonics.
    // Unbiased (divide by N-lag) tends to make fundamental and
    // subharmonic equally strong for regular kick patterns.
    let n = ds_dc.len();
    let mut acf = vec![0.0f64; max_lag + 2];
    let norm = n as f64;
    for lag in min_lag..=max_lag {
        let mut s = 0.0f64;
        for i in 0..n - lag {
            s += (ds_dc[i] as f64) * (ds_dc[i + lag] as f64);
        }
        acf[lag] = s / norm;
    }

    // Local-maxima peak list.
    let mut peaks: Vec<(usize, f64)> = Vec::new();
    for lag in (min_lag + 1)..max_lag {
        if acf[lag] >= acf[lag - 1] && acf[lag] >= acf[lag + 1] && acf[lag] > 0.0 {
            peaks.push((lag, acf[lag]));
        }
    }
    if peaks.is_empty() {
        return None;
    }
    peaks.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let peak1_val = peaks[0].1;
    let peak2_val = peaks.get(1).map(|p| p.1).unwrap_or(0.0);
    let confidence_ratio = if peak2_val > 0.0 {
        (peak1_val / peak2_val) as f32
    } else {
        3.0
    };

    // Gate: sparse clips with weak ACF are rejected rather than
    // guessed. This is the difference between "we detected nothing"
    // and "we detected garbage".
    let onsets_count = detect_onsets(audio, 1.5).len();
    if onsets_count < 8 && confidence_ratio < 1.5 {
        return None;
    }

    // Pick the single strongest ACF peak. Sub-lag-accurate via
    // parabolic interpolation around the neighbours. Then Parncutt-
    // fold only if the raw lag sits outside the target octave (or is
    // suspiciously slow, where the track could be a 2-bar subharmonic
    // of a dance-music tempo).
    let (best_lag, _) = peaks[0];
    let refined_lag = if best_lag > min_lag && best_lag < max_lag {
        best_lag as f64 + parabolic_vertex(acf[best_lag - 1], acf[best_lag], acf[best_lag + 1])
    } else {
        best_lag as f64
    };
    if refined_lag <= 0.0 {
        return None;
    }
    let raw_bpm = 60.0 * odf_sr / refined_lag;
    let best_bpm = fold_to_preferred_octave(raw_bpm);
    if !best_bpm.is_finite() || best_bpm <= 0.0 {
        return None;
    }

    let confidence = (confidence_ratio / 3.0).clamp(0.0, 1.0);
    Some(BpmEstimate {
        bpm: best_bpm,
        confidence,
    })
}

fn parncutt_weight(bpm: f64) -> f64 {
    // Log-domain Gaussian centred on 120 BPM. sigma=0.8 gives a gentle
    // shoulder so 100 and 140 are both close to peak weight.
    let log_ratio = (bpm / BPM_FOLD_PREF).log2();
    let sigma = 0.8;
    (-0.5 * (log_ratio / sigma).powi(2)).exp()
}

fn fold_to_preferred_octave(bpm: f64) -> f64 {
    if !bpm.is_finite() || bpm <= 0.0 {
        return BPM_FOLD_PREF;
    }
    // Inside the preferred range: trust the raw peak, except when the
    // detection is suspiciously slow (≤ 90 BPM). In that case consider
    // its double, since dance-music tracks often produce a dominant
    // ACF peak at the 2-bar period. Parncutt weight on log(bpm/120)
    // picks whichever octave is more musically likely.
    if (BPM_FOLD_LO..=BPM_FOLD_HI).contains(&bpm) {
        if bpm <= 90.0 {
            let doubled = bpm * 2.0;
            if (BPM_FOLD_LO..=BPM_FOLD_HI).contains(&doubled) {
                let w1 = parncutt_weight(bpm);
                let w2 = parncutt_weight(doubled);
                return if w2 > w1 { doubled } else { bpm };
            }
        }
        return bpm;
    }
    // Outside the preferred range: Parncutt-fold among ×0.25…×4.
    let candidates = [bpm * 0.25, bpm * 0.5, bpm, bpm * 2.0, bpm * 4.0];
    let mut best = bpm;
    let mut best_w = -1.0;
    for &c in &candidates {
        if (BPM_FOLD_LO..=BPM_FOLD_HI).contains(&c) {
            let w = parncutt_weight(c);
            if w > best_w {
                best_w = w;
                best = c;
            }
        }
    }
    if best_w < 0.0 {
        let mut b = bpm;
        while b < BPM_FOLD_LO && b > 0.0 {
            b *= 2.0;
        }
        while b > BPM_FOLD_HI {
            b /= 2.0;
        }
        return b;
    }
    best
}

fn parabolic_vertex(y_m: f64, y_0: f64, y_p: f64) -> f64 {
    let denom = y_m - 2.0 * y_0 + y_p;
    if denom.abs() < 1e-12 {
        0.0
    } else {
        (0.5 * (y_m - y_p) / denom).clamp(-1.0, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_audio(channels: Vec<Vec<f32>>, sr: u32) -> DecodedAudio {
        DecodedAudio {
            channels,
            sample_rate: sr,
        }
    }

    fn impulse_train(sr: u32, intervals_frames: &[usize]) -> DecodedAudio {
        let total: usize = intervals_frames.iter().sum::<usize>() + 1024;
        let mut buf = vec![0.0f32; total];
        let mut pos = 512usize;
        for &gap in intervals_frames {
            if pos < buf.len() {
                buf[pos] = 1.0;
                if pos + 1 < buf.len() {
                    buf[pos + 1] = -0.8;
                }
            }
            pos += gap;
        }
        make_audio(vec![buf.clone(), buf], sr)
    }

    #[test]
    fn empty_audio_returns_no_onsets() {
        let audio = make_audio(vec![], 44_100);
        assert!(detect_onsets(&audio, 1.5).is_empty());
    }

    #[test]
    fn very_short_audio_returns_no_onsets() {
        let audio = make_audio(vec![vec![0.5; 20], vec![0.5; 20]], 44_100);
        assert!(detect_onsets(&audio, 1.5).is_empty());
    }

    #[test]
    fn silence_produces_no_onsets() {
        let audio = make_audio(vec![vec![0.0; 22_050], vec![0.0; 22_050]], 44_100);
        assert!(detect_onsets(&audio, 1.5).is_empty());
    }

    #[test]
    fn constant_signal_has_only_a_start_onset() {
        // A DC step is a legitimate transient at the attack, so we expect at
        // most one onset, located near the start, and none mid-signal.
        let audio = make_audio(vec![vec![0.25; 44_100], vec![0.25; 44_100]], 44_100);
        let onsets = detect_onsets(&audio, 1.5);
        assert!(onsets.len() <= 1, "got {:?}", onsets);
        if let Some(&first) = onsets.first() {
            assert!(first < 4_410, "unexpected mid-signal onset at {first}");
        }
    }

    #[test]
    fn impulse_train_detects_hits() {
        // 8 hits at 8820-sample spacing = 200ms at 44.1kHz.
        let audio = impulse_train(44_100, &[8_820; 8]);
        let onsets = detect_onsets(&audio, 1.2);
        assert!(
            onsets.len() >= 6,
            "expected ~8 detections, got {}: {:?}",
            onsets.len(),
            onsets
        );
        assert!(
            onsets.len() <= 10,
            "expected ~8 detections, got {}: {:?}",
            onsets.len(),
            onsets
        );
    }

    #[test]
    fn refractory_prevents_double_triggers() {
        // Two spikes 10ms apart: should collapse to one onset (refractory 30ms).
        let sr = 44_100u32;
        let mut buf = vec![0.0f32; 22_050];
        buf[5_000] = 1.0;
        buf[5_001] = -0.8;
        buf[5_441] = 1.0; // 10ms later
        buf[5_442] = -0.8;
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let onsets = detect_onsets(&audio, 1.0);
        assert!(
            onsets.len() <= 1,
            "refractory violated: {} onsets {:?}",
            onsets.len(),
            onsets
        );
    }

    #[test]
    fn onsets_land_near_impulses() {
        let sr = 44_100u32;
        let mut buf = vec![0.0f32; 44_100];
        let expected = [5_000usize, 15_000, 25_000, 35_000];
        for &pos in &expected {
            buf[pos] = 1.0;
            buf[pos + 1] = -0.8;
        }
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let onsets = detect_onsets(&audio, 1.2);
        assert!(!onsets.is_empty());
        for exp in expected.iter() {
            let found = onsets
                .iter()
                .any(|&o| (o as i64 - *exp as i64).abs() < 512);
            assert!(found, "no onset within 512 samples of {}: {:?}", exp, onsets);
        }
    }

    #[test]
    fn sensitivity_affects_yield() {
        // Low-amplitude impulses: high sensitivity should reject them.
        let sr = 44_100u32;
        let mut buf = vec![0.0f32; 44_100];
        for i in (2_000..40_000).step_by(5_000) {
            buf[i] = 0.05;
        }
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let loose = detect_onsets(&audio, 1.0).len();
        let tight = detect_onsets(&audio, 3.0).len();
        assert!(
            tight <= loose,
            "tight {} should not exceed loose {}",
            tight,
            loose
        );
    }

    fn synthetic_kick_track(sr: u32, bpm: f64, duration_sec: f64) -> DecodedAudio {
        let period = (60.0 / bpm * sr as f64) as usize;
        let total = (duration_sec * sr as f64) as usize;
        let mut buf = vec![0.0f32; total];
        let mut pos = 0usize;
        let kick_len = sr as usize / 20; // 50 ms kick tail
        while pos < total {
            for i in 0..kick_len {
                if pos + i >= total {
                    break;
                }
                let t = i as f32 / sr as f32;
                let env = (-t * 60.0).exp();
                let freq = 80.0 - 50.0 * t; // slight pitch drop
                let s = (2.0 * std::f32::consts::PI * freq * t).sin();
                buf[pos + i] += s * env * 0.9;
            }
            pos += period;
        }
        make_audio(vec![buf.clone(), buf], sr)
    }

    fn dense_drum_track(sr: u32, bpm: f64, duration_sec: f64) -> DecodedAudio {
        let beat = (60.0 / bpm * sr as f64) as usize;
        let total = (duration_sec * sr as f64) as usize;
        let mut buf = vec![0.0f32; total];
        let place_kick = |buf: &mut [f32], pos: usize, amp: f32, low: bool| {
            let kick_len = sr as usize / 20;
            for i in 0..kick_len {
                if pos + i >= buf.len() {
                    break;
                }
                let t = i as f32 / sr as f32;
                let env = (-t * 80.0).exp();
                let freq = if low { 60.0 - 40.0 * t } else { 180.0 };
                let s = (2.0 * std::f32::consts::PI * freq * t).sin();
                buf[pos + i] += s * env * amp;
            }
        };
        let place_hat = |buf: &mut [f32], pos: usize, amp: f32| {
            let hat_len = sr as usize / 100;
            for i in 0..hat_len {
                if pos + i >= buf.len() {
                    break;
                }
                let t = i as f32 / sr as f32;
                let env = (-t * 400.0).exp();
                // cheap white noise via trig
                let s = ((i as f32 * 7919.0).sin() * 0.7 + (i as f32 * 12421.0).cos() * 0.3).tanh();
                buf[pos + i] += s * env * amp;
            }
        };

        let mut beat_idx = 0usize;
        let mut pos = 0usize;
        while pos < total {
            // Kick on every beat.
            place_kick(&mut buf, pos, 0.9, true);
            // Snare on beats 2 and 4.
            if beat_idx % 2 == 1 {
                place_kick(&mut buf, pos, 0.7, false);
            }
            // Hats on eighths.
            place_hat(&mut buf, pos, 0.4);
            place_hat(&mut buf, pos + beat / 2, 0.4);
            pos += beat;
            beat_idx += 1;
        }
        make_audio(vec![buf.clone(), buf], sr)
    }

    fn sustained_sine(sr: u32, freq: f32, duration_sec: f64) -> DecodedAudio {
        let total = (duration_sec * sr as f64) as usize;
        let buf: Vec<f32> = (0..total)
            .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr as f32).sin() * 0.4)
            .collect();
        make_audio(vec![buf.clone(), buf], sr)
    }

    #[test]
    fn detects_120_bpm() {
        let audio = synthetic_kick_track(44_100, 120.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 120.0).abs() < 1.0,
            "expected ~120 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn detects_100_bpm() {
        let audio = synthetic_kick_track(44_100, 100.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 100.0).abs() < 1.0,
            "expected ~100 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn detects_140_bpm() {
        let audio = synthetic_kick_track(44_100, 140.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 140.0).abs() < 1.0,
            "expected ~140 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn detects_174_bpm() {
        let audio = synthetic_kick_track(44_100, 174.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 174.0).abs() < 1.5,
            "expected ~174 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn resolves_dense_drums_to_right_octave() {
        let audio = dense_drum_track(44_100, 128.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        // Hats on eighths mean a strong ACF peak at 256 BPM. Octave
        // folding should pull this back to 128, not 64 or 256.
        assert!(
            (est.bpm - 128.0).abs() < 2.0,
            "expected ~128 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn silence_returns_none() {
        let audio = make_audio(vec![vec![0.0; 44_100 * 4]; 2], 44_100);
        assert!(detect_bpm(&audio, 44_100).is_none());
    }

    #[test]
    fn sustained_sine_returns_none() {
        let audio = sustained_sine(44_100, 220.0, 4.0);
        // A flat pad has no rhythm; the detector should admit that
        // rather than hallucinating a BPM.
        assert!(detect_bpm(&audio, 44_100).is_none());
    }

    #[test]
    fn very_short_audio_returns_none() {
        // 0.5 s of silence is well below our 1-second floor.
        let audio = make_audio(vec![vec![0.1; 22_050]; 2], 44_100);
        assert!(detect_bpm(&audio, 44_100).is_none());
    }

    #[test]
    fn detects_single_bar_at_122_bpm() {
        // A single bar at 122 BPM is ~1.97 s. The old 2-second
        // floor rejected this outright; lowering to 1 s lets us
        // pick up the tempo from 4 beats in a single bar.
        let audio = synthetic_kick_track(44_100, 122.0, 1.97);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 122.0).abs() < 2.0,
            "expected ~122 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn detects_single_bar_at_140_bpm() {
        // 1 bar at 140 BPM ≈ 1.71 s.
        let audio = synthetic_kick_track(44_100, 140.0, 1.71);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 140.0).abs() < 2.0,
            "expected ~140 BPM, got {}",
            est.bpm
        );
    }

    #[test]
    fn confidence_in_unit_range() {
        let audio = synthetic_kick_track(44_100, 120.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(est.confidence >= 0.0 && est.confidence <= 1.0);
    }
}
