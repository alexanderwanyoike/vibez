//! Offline transient / onset detection for audio clips.
//!
//! Pure time-domain detector suited for drum and percussion material:
//! high-pass pre-emphasis, full-wave rectification, one-pole envelope,
//! positive-flux onset-detection function, adaptive threshold, and
//! peak picking with a refractory window. No external deps.
//!
//! Returns `Vec<u64>` of absolute frame indices within the input audio.
//! Good enough to drive slice-and-snap audio quantize.

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

    let mut onsets = Vec::new();
    let mut last_peak: Option<usize> = None;

    for i in 1..frames.saturating_sub(1) {
        let window_start = i.saturating_sub(threshold_window);
        let window = &odf[window_start..i];
        if window.is_empty() {
            continue;
        }
        let (mean, std) = mean_std(window);
        let threshold = mean + sensitivity * std.max(1e-6);

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

fn mean_std(window: &[f32]) -> (f32, f32) {
    let n = window.len() as f32;
    let mean = window.iter().sum::<f32>() / n;
    let variance = window.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / n;
    (mean, variance.sqrt())
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
}
