//! Offline transient / onset detection and BPM estimation for audio
//! clips.
//!
//! Transient event candidates use a Rust port of aubio 0.4.9's established
//! complex-domain onset pipeline. Accepted event peaks are backtracked to an
//! energy minimum so editing markers land at slice boundaries. Tempo
//! estimation retains a separate onset-envelope signal for autocorrelation.
//!
//! Two entrypoints:
//! - `detect_onsets` returns `Vec<u64>` of absolute frame indices.
//!   Drives slice-and-snap audio quantize.
//! - `detect_bpm` returns an `Option<BpmEstimate>`. Autocorrelates the
//!   onset flux (so it works on sustained melodic material, not just
//!   percussion), octave-folds into [60, 200] BPM using Parncutt's
//!   preference curve, and gates on confidence so sparse or silent
//!   clips return `None` rather than guessing.

use crate::audio_buffer::DecodedAudio;

mod aubio;
#[cfg(test)]
mod transient_analysis_tests;

/// Producer-facing transient sensitivity. Higher percentages retain quieter
/// attacks; lower percentages keep only the most prominent attacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct TransientSensitivity(u8);

impl TransientSensitivity {
    pub const MIN_PERCENT: u8 = 0;
    pub const MAX_PERCENT: u8 = 100;
    pub const DEFAULT: Self = Self(50);

    pub const fn new(percent: u8) -> Self {
        Self(if percent > Self::MAX_PERCENT {
            Self::MAX_PERCENT
        } else {
            percent
        })
    }

    pub const fn percent(self) -> u8 {
        self.0
    }

    pub(crate) fn normalized(self) -> f32 {
        self.0 as f32 / Self::MAX_PERCENT as f32
    }

    #[cfg(test)]
    fn threshold_through(self, midpoint: f32) -> f32 {
        let normalized = self.normalized();
        if normalized <= 0.5 {
            0.9 * (midpoint / 0.9).powf(normalized / 0.5)
        } else {
            midpoint * (0.001 / midpoint).powf((normalized - 0.5) / 0.5)
        }
    }
}

impl Default for TransientSensitivity {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Sample-count used to measure envelope-level change (onset function).
const FLUX_WINDOW: usize = 64;

/// Envelope follower time constants.
const ATTACK_MS: f32 = 5.0;
const RELEASE_MS: f32 = 50.0;

/// High-pass pre-emphasis coefficient.
const PREEMPHASIS: f32 = 0.97;

/// Detect onsets in `audio` and return sample indices.
///
/// Candidate generation is fixed so sensitivity can only add or remove stable
/// source-frame positions. Higher percentages retain quieter attacks.
pub fn detect_onsets(audio: &DecodedAudio, sensitivity: TransientSensitivity) -> Vec<u64> {
    let frames = audio.num_frames();
    if frames < 1_024 || audio.sample_rate == 0 {
        return Vec::new();
    }
    let config = aubio::Config::for_complex_candidates(audio.sample_rate);
    // Analyse channels independently. Averaging stereo before analysis can
    // erase an attack when channels carry opposite-polarity content.
    let mut candidates = audio
        .channels
        .iter()
        .flat_map(|channel| {
            let analysis_delay = config.analysis_delay_frames();
            aubio::detect_complex_onsets(channel, audio.sample_rate, config)
                .into_iter()
                .map(move |estimated_onset| {
                    (
                        estimated_onset,
                        estimated_onset.saturating_add(analysis_delay),
                    )
                })
        })
        .collect::<Vec<_>>();

    // Aubio's spectral descriptors can report a pitched decay cycle as a new
    // onset. Transient markers are editing boundaries, so require each
    // spectral candidate to coincide with a meaningful time-domain attack.
    let global_peak = audio
        .channels
        .iter()
        .flat_map(|channel| channel.iter())
        .map(|sample| sample.abs())
        .fold(0.0f32, f32::max);
    candidates.retain(|&(estimated_onset, _)| {
        estimated_onset > 0 && genuine_attack(audio, estimated_onset, sensitivity, global_peak)
    });
    // Aubio subtracts its analysis delay to estimate the musical onset. For
    // slice localisation we instead begin at the later detected peak, then
    // backtrack through source energy to the actual boundary. Starting from
    // the compensated estimate can put an impulse before its own attack.
    let peaks = candidates
        .into_iter()
        .map(|(_, detected_peak)| detected_peak)
        .collect::<Vec<_>>();
    let mut onsets = backtrack_to_energy_minima(audio, &peaks);
    // Clip start is already an explicit boundary in Vibez. Aubio reports a
    // non-silent file start as an onset, but showing that as a suggested
    // transient marker adds no editable information.
    onsets.retain(|&frame| frame > 0 && frame < frames as u64);
    canonicalize_onsets(
        &mut onsets,
        config.minimum_inter_onset_frames(audio.sample_rate),
    )
}

fn canonicalize_onsets(onsets: &mut [u64], minimum_interval_frames: u64) -> Vec<u64> {
    onsets.sort_unstable();
    let mut canonical = Vec::with_capacity(onsets.len());
    for &onset in onsets.iter() {
        if canonical
            .last()
            .is_none_or(|previous: &u64| onset.abs_diff(*previous) > minimum_interval_frames)
        {
            canonical.push(onset);
        }
    }
    canonical
}

fn frames_for_ms(sample_rate: u32, milliseconds: f32) -> usize {
    (sample_rate as f32 * milliseconds / 1_000.0).round() as usize
}

fn genuine_attack(
    audio: &DecodedAudio,
    frame: u64,
    sensitivity: TransientSensitivity,
    global_peak: f32,
) -> bool {
    if global_peak <= f32::EPSILON {
        return false;
    }

    let frame = usize::try_from(frame).unwrap_or(usize::MAX);
    let pre_start = frame.saturating_sub(frames_for_ms(audio.sample_rate, 35.0));
    let pre_end = frame.saturating_sub(frames_for_ms(audio.sample_rate, 5.0));
    let post_end = frame
        .saturating_add(frames_for_ms(audio.sample_rate, 35.0))
        .min(audio.num_frames());
    let (_, pre_rms) = peak_and_rms(audio, pre_start, pre_end);
    let (post_peak, post_rms) = peak_and_rms(audio, frame, post_end);

    post_peak >= global_peak * attack_prominence_floor(sensitivity) && post_rms > pre_rms * 1.05
}

fn peak_and_rms(audio: &DecodedAudio, start: usize, end: usize) -> (f32, f32) {
    let mut peak = 0.0f32;
    let mut square_sum = 0.0f64;
    let mut sample_count = 0usize;
    for channel in &audio.channels {
        for &sample in channel.get(start..end).unwrap_or_default() {
            peak = peak.max(sample.abs());
            square_sum += f64::from(sample) * f64::from(sample);
            sample_count += 1;
        }
    }
    let rms = if sample_count == 0 {
        0.0
    } else {
        (square_sum / sample_count as f64).sqrt() as f32
    };
    (peak, rms)
}

const BACKTRACK_WINDOW_MS: f32 = 23.22;
const BACKTRACK_HOP_MS: f32 = 2.9;
const BACKTRACK_MAX_MS: f32 = 50.0;

fn backtrack_to_energy_minima(audio: &DecodedAudio, onsets: &[u64]) -> Vec<u64> {
    if onsets.is_empty() || audio.num_frames() < 3 {
        return onsets.to_vec();
    }

    let hop = frames_for_ms(audio.sample_rate, BACKTRACK_HOP_MS).max(1);
    let window = frames_for_ms(audio.sample_rate, BACKTRACK_WINDOW_MS).max(1);
    let energy = rms_energy_envelope(audio, window, hop);
    backtrack_events_to_minima(
        onsets,
        &energy,
        hop,
        frames_for_ms(audio.sample_rate, BACKTRACK_MAX_MS),
    )
}

fn rms_energy_envelope(audio: &DecodedAudio, window: usize, hop: usize) -> Vec<f32> {
    let frames = audio.num_frames();
    if frames == 0 || window == 0 || hop == 0 {
        return Vec::new();
    }

    let channel_count = audio.channels.len().max(1) as f64;
    let mut prefix_energy = vec![0.0f64; frames + 1];
    for frame in 0..frames {
        let square_sum = audio
            .channels
            .iter()
            .map(|channel| f64::from(channel.get(frame).copied().unwrap_or(0.0)).powi(2))
            .sum::<f64>();
        prefix_energy[frame + 1] = prefix_energy[frame] + square_sum / channel_count;
    }

    (0..frames)
        .step_by(hop)
        .map(|end| {
            // A trailing RMS window keeps the final silence minimum aligned
            // with the source attack. A centred window would smear future
            // attack energy earlier by half a window and make slices early.
            let start = end.saturating_sub(window);
            let frame_count = (end - start).max(1) as f64;
            ((prefix_energy[end] - prefix_energy[start]) / frame_count).sqrt() as f32
        })
        .collect()
}

// The local-minimum matching rule is derived from librosa 0.11.0's
// `onset_backtrack`, copyright (c) 2013-2023 the librosa development team,
// distributed under the ISC licence:
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
//
// Vibez adds a bounded look-back expressed in source frames because a DAW
// marker must not jump into an earlier event.
fn backtrack_events_to_minima(
    events: &[u64],
    energy: &[f32],
    hop: usize,
    max_lookback: usize,
) -> Vec<u64> {
    if energy.len() < 3 || hop == 0 {
        return events.to_vec();
    }

    let mut minima = vec![0usize];
    minima.extend(energy.windows(3).enumerate().filter_map(|(index, values)| {
        (values[1] <= values[0] && values[1] < values[2]).then_some(index + 1)
    }));

    events
        .iter()
        .map(|&event| {
            let event = usize::try_from(event).unwrap_or(usize::MAX);
            let first_allowed = event.saturating_sub(max_lookback);
            minima
                .iter()
                .rev()
                .map(|minimum| minimum.saturating_mul(hop))
                .find(|minimum| *minimum <= event && *minimum >= first_allowed)
                .map_or(event as u64, |minimum| minimum as u64)
        })
        .collect()
}

fn attack_prominence_floor(sensitivity: TransientSensitivity) -> f32 {
    let normalized = sensitivity.normalized();
    if normalized <= 0.5 {
        0.6 * (0.1f32 / 0.6).powf(normalized / 0.5)
    } else {
        0.1 * (0.005f32 / 0.1).powf((normalized - 0.5) / 0.5)
    }
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
// Keep aligned with vibez-ui's PLAUSIBLE_LOOP_BPM_MAX so Browser fitting does
// not accept a source tempo which onset analysis has already folded in half.
const BPM_FOLD_HI: f64 = 200.0;
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
/// commit to a tempo. Output BPM is always in `[60, 200]` after
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
    let onsets_count = detect_onsets(audio, TransientSensitivity::DEFAULT).len();
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

    fn burst_train(sr: u32, intervals_frames: &[usize]) -> DecodedAudio {
        let total: usize = intervals_frames.iter().sum::<usize>() + 1024;
        let mut buf = vec![0.0f32; total];
        let mut pos = 512usize;
        for &gap in intervals_frames {
            for offset in 0..1_024 {
                if pos + offset >= buf.len() {
                    break;
                }
                let phase = std::f32::consts::TAU * 120.0 * offset as f32 / sr as f32;
                buf[pos + offset] += phase.sin() * (-(offset as f32) / 300.0).exp() * 0.8;
            }
            pos += gap;
        }
        make_audio(vec![buf.clone(), buf], sr)
    }

    #[test]
    fn empty_audio_returns_no_onsets() {
        let audio = make_audio(vec![], 44_100);
        assert!(detect_onsets(&audio, TransientSensitivity::DEFAULT).is_empty());
    }

    #[test]
    fn very_short_audio_returns_no_onsets() {
        let audio = make_audio(vec![vec![0.5; 20], vec![0.5; 20]], 44_100);
        assert!(detect_onsets(&audio, TransientSensitivity::DEFAULT).is_empty());
    }

    #[test]
    fn silence_produces_no_onsets() {
        let audio = make_audio(vec![vec![0.0; 22_050], vec![0.0; 22_050]], 44_100);
        assert!(detect_onsets(&audio, TransientSensitivity::DEFAULT).is_empty());
    }

    #[test]
    fn constant_signal_has_only_a_start_onset() {
        // A DC step is a legitimate transient at the attack, so we expect at
        // most one onset, located near the start, and none mid-signal.
        let audio = make_audio(vec![vec![0.25; 44_100], vec![0.25; 44_100]], 44_100);
        let onsets = detect_onsets(&audio, TransientSensitivity::DEFAULT);
        assert!(onsets.len() <= 1, "got {:?}", onsets);
        if let Some(&first) = onsets.first() {
            assert!(first < 4_410, "unexpected mid-signal onset at {first}");
        }
    }

    #[test]
    fn impulse_train_detects_hits() {
        // 8 hits at 8820-sample spacing = 200ms at 44.1kHz.
        let audio = burst_train(44_100, &[8_820; 8]);
        let onsets = detect_onsets(&audio, TransientSensitivity::new(60));
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
    fn strong_sixteenth_note_attacks_are_not_suppressed_by_sensitivity() {
        // Sixteenth notes at 120 BPM are 125 ms apart. Sensitivity may reject
        // quieter attacks, but it must not impose a longer timing gap that
        // blindly removes every other strong hit.
        let audio = burst_train(44_100, &[5_512; 8]);
        let onsets = detect_onsets(&audio, TransientSensitivity::new(0));
        assert!(
            onsets.len() >= 7,
            "expected the eight strong attacks to survive, got {onsets:?}"
        );
    }

    #[test]
    fn refractory_prevents_double_triggers() {
        // Two spikes 10ms apart: should collapse to one onset (refractory 30ms).
        let sr = 44_100u32;
        let mut buf = vec![0.0f32; 22_050];
        for pos in [5_000usize, 5_441] {
            for offset in 0..1_024 {
                let phase = std::f32::consts::TAU * 120.0 * offset as f32 / sr as f32;
                buf[pos + offset] += phase.sin() * (-(offset as f32) / 300.0).exp() * 0.8;
            }
        }
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let onsets = detect_onsets(&audio, TransientSensitivity::new(70));
        assert!(
            onsets.len() == 1,
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
            for offset in 0..1_024 {
                let phase = std::f32::consts::TAU * 120.0 * offset as f32 / sr as f32;
                buf[pos + offset] += phase.sin() * (-(offset as f32) / 300.0).exp() * 0.8;
            }
        }
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let onsets = detect_onsets(&audio, TransientSensitivity::new(60));
        assert!(!onsets.is_empty());
        for exp in expected.iter() {
            let found = onsets.iter().any(|&o| (o as i64 - *exp as i64).abs() < 512);
            assert!(
                found,
                "no onset within 512 samples of {}: {:?}",
                exp, onsets
            );
        }
    }

    #[test]
    fn onset_detection_is_deterministic_for_identical_audio() {
        let audio = burst_train(44_100, &[8_820; 8]);
        assert_eq!(
            detect_onsets(&audio, TransientSensitivity::DEFAULT),
            detect_onsets(&audio, TransientSensitivity::DEFAULT)
        );
    }

    #[test]
    fn sensitivity_affects_yield() {
        // A clip with attacks at different levels must produce fewer markers
        // at the strong-attacks end and progressively retain quieter attacks
        // as sensitivity increases. Equal result sets would make the producer
        // control a no-op even though they technically remain nested.
        let sr = 44_100u32;
        let mut buf = vec![0.0f32; 44_100];
        for (index, amplitude) in [1.0f32, 0.5, 0.2, 0.08, 0.02].into_iter().enumerate() {
            let start = 2_000 + index * 8_000;
            for offset in 0..1_024 {
                let phase = std::f32::consts::TAU * 120.0 * offset as f32 / sr as f32;
                buf[start + offset] += phase.sin() * (-(offset as f32) / 300.0).exp() * amplitude;
            }
        }
        let audio = make_audio(vec![buf.clone(), buf], sr);
        let results = [0, 25, 50, 75, 100]
            .map(|percent| detect_onsets(&audio, TransientSensitivity::new(percent)));
        for pair in results.windows(2) {
            assert!(
                pair[0].iter().all(|onset| pair[1].contains(onset)),
                "higher sensitivity {:?} must retain every lower-sensitivity marker {:?}",
                pair[1],
                pair[0],
            );
        }
        assert!(
            results[0].len() < results[4].len(),
            "sensitivity endpoints must differ: {results:?}"
        );
        assert!(
            results.windows(2).filter(|pair| pair[0] != pair[1]).count() >= 2,
            "sensitivity must provide more than one useful step: {results:?}"
        );
    }

    #[test]
    fn canonical_onsets_merge_cross_channel_candidates_within_aubio_minimum_interval() {
        let sample_rate = 44_100u32;
        let first = 10_000u64;
        let mut onsets = vec![first, first + frames_for_ms(sample_rate, 7.0) as u64];
        let minimum_interval = aubio::Config::for_complex_candidates(sample_rate)
            .minimum_inter_onset_frames(sample_rate);

        assert_eq!(
            canonicalize_onsets(&mut onsets, minimum_interval),
            vec![first]
        );
    }

    #[test]
    fn default_sensitivity_rejects_quiet_decay_ripples() {
        let sr = 44_100u32;
        let mut channel = vec![0.0f32; 22_050];
        let strong = 4_000usize;
        let ripple = 12_000usize;
        for (start, amplitude) in [(strong, 1.0f32), (ripple, 0.04f32)] {
            for offset in 0..1_024 {
                let phase = std::f32::consts::TAU * 180.0 * offset as f32 / sr as f32;
                channel[start + offset] =
                    phase.sin() * (-(offset as f32) / 300.0).exp() * amplitude;
            }
        }
        let audio = make_audio(vec![channel.clone(), channel], sr);
        let global_peak = 1.0;

        assert!(genuine_attack(
            &audio,
            strong as u64,
            TransientSensitivity::DEFAULT,
            global_peak,
        ));
        assert!(!genuine_attack(
            &audio,
            ripple as u64,
            TransientSensitivity::DEFAULT,
            global_peak,
        ));
        assert!(genuine_attack(
            &audio,
            ripple as u64,
            TransientSensitivity::new(100),
            global_peak,
        ));
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
    fn detects_190_bpm_without_folding_to_half_time() {
        let audio = synthetic_kick_track(44_100, 190.0, 8.0);
        let est = detect_bpm(&audio, 44_100).expect("expected detection");
        assert!(
            (est.bpm - 190.0).abs() < 2.0,
            "expected ~190 BPM, got {}",
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
