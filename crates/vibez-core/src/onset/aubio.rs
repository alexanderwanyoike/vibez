//! Rust port of aubio's offline onset detector.
//!
//! The detector, peak picker, constants, and processing order in this module
//! are derived from aubio 0.4.9. The FFT implementation is provided by
//! `rustfft`; Vibez does not port aubio's platform-specific FFT backends.
//!
//! Original work copyright (C) 2003-2014 Paul Brossier <piem@aubio.org>.
//! aubio is licensed under GPL-3.0-or-later. Vibez carries the same compatible
//! licence. Upstream source: <https://github.com/aubio/aubio/tree/0.4.9>.

use super::TransientSensitivity;
use rustfft::{num_complex::Complex32, Fft, FftPlanner};
use std::sync::Arc;

const DEFAULT_WINDOW_SIZE: usize = 1_024;
const DEFAULT_HOP_SIZE: usize = 256;
// aubio's documented complex-domain default. Sensitivity interpolates through
// this value at 50%, from the documented 0.900..=0.001 useful range.
const COMPLEX_DEFAULT_THRESHOLD: f32 = 0.15;
const DEFAULT_MIN_IOI_MS: f32 = 50.0;
const DEFAULT_SILENCE_DB: f32 = -70.0;
const DEFAULT_DELAY_HOPS: f32 = 4.3;
const COMPLEX_DELAY_HOPS: f32 = 4.6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Config {
    window_size: usize,
    hop_size: usize,
    threshold: f32,
    min_ioi_ms: f32,
    silence_db: f32,
    delay_hops: f32,
}

impl Config {
    pub(super) fn for_complex_candidates(
        sample_rate: u32,
        sensitivity: TransientSensitivity,
    ) -> Self {
        let window_size = match sample_rate {
            0..=33_074 => DEFAULT_WINDOW_SIZE / 2,
            33_075..=66_149 => DEFAULT_WINDOW_SIZE,
            66_150..=132_299 => DEFAULT_WINDOW_SIZE * 2,
            _ => DEFAULT_WINDOW_SIZE * 4,
        };
        Self {
            threshold: sensitivity.threshold_through(COMPLEX_DEFAULT_THRESHOLD),
            delay_hops: COMPLEX_DELAY_HOPS,
            window_size,
            hop_size: window_size / 4,
            ..Self::default()
        }
    }

    pub(super) fn minimum_inter_onset_frames(self, sample_rate: u32) -> u64 {
        ((self.min_ioi_ms / 1_000.0) * sample_rate as f32).round() as u64
    }

    fn validate(self, sample_rate: u32) -> Result<Self, ConfigError> {
        if self.hop_size == 0 {
            return Err(ConfigError::ZeroHopSize);
        }
        if self.window_size < 2 {
            return Err(ConfigError::WindowTooSmall);
        }
        if self.window_size < self.hop_size {
            return Err(ConfigError::HopLargerThanWindow);
        }
        if sample_rate == 0 {
            return Err(ConfigError::ZeroSampleRate);
        }
        Ok(self)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_size: DEFAULT_WINDOW_SIZE,
            hop_size: DEFAULT_HOP_SIZE,
            threshold: COMPLEX_DEFAULT_THRESHOLD,
            min_ioi_ms: DEFAULT_MIN_IOI_MS,
            silence_db: DEFAULT_SILENCE_DB,
            delay_hops: DEFAULT_DELAY_HOPS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigError {
    ZeroHopSize,
    WindowTooSmall,
    HopLargerThanWindow,
    ZeroSampleRate,
}

pub(super) fn detect_complex_onsets_where(
    mono: &[f32],
    sample_rate: u32,
    config: Config,
    accepts: impl FnMut(u64) -> bool,
) -> Vec<(u64, u64)> {
    detect_onsets_where(mono, sample_rate, config, accepts)
}

fn detect_onsets_where(
    mono: &[f32],
    sample_rate: u32,
    config: Config,
    accepts: impl FnMut(u64) -> bool,
) -> Vec<(u64, u64)> {
    let Ok(config) = config.validate(sample_rate) else {
        return Vec::new();
    };
    if mono.is_empty() {
        return Vec::new();
    }

    Detector::new(sample_rate, config).process(mono, accepts)
}

struct Detector {
    hop_size: usize,
    min_ioi: usize,
    delay: usize,
    silence_db: f32,
    total_frames: usize,
    last_onset: usize,
    phase_vocoder: PhaseVocoder,
    peak_picker: PeakPicker,
    descriptor: ComplexDomain,
}

impl Detector {
    fn new(sample_rate: u32, config: Config) -> Self {
        let bins = config.window_size / 2 + 1;
        Self {
            hop_size: config.hop_size,
            min_ioi: ((config.min_ioi_ms / 1_000.0) * sample_rate as f32).round() as usize,
            delay: (config.delay_hops * config.hop_size as f32) as usize,
            silence_db: config.silence_db,
            total_frames: 0,
            last_onset: 0,
            phase_vocoder: PhaseVocoder::new(config.window_size, config.hop_size),
            peak_picker: PeakPicker::new(config.threshold),
            descriptor: ComplexDomain::new(bins, sample_rate, config.hop_size),
        }
    }

    fn process(mut self, mono: &[f32], mut accepts: impl FnMut(u64) -> bool) -> Vec<(u64, u64)> {
        let mut onsets = Vec::new();
        let mut hop = vec![0.0; self.hop_size];

        for chunk in mono.chunks(self.hop_size) {
            hop.fill(0.0);
            hop[..chunk.len()].copy_from_slice(chunk);
            if let Some(event) = self.process_hop(&hop, &mut accepts) {
                onsets.push(event);
            }
        }
        onsets
    }

    fn process_hop(
        &mut self,
        input: &[f32],
        accepts: &mut impl FnMut(u64) -> bool,
    ) -> Option<(u64, u64)> {
        let spectrum = self.phase_vocoder.spectrum(input);
        let descriptor = self.descriptor.process(&spectrum);
        let peak = self.peak_picker.process(descriptor);
        let silent = db_spl(input) < self.silence_db;
        let mut accepted = None;

        if peak > 0.0 && !silent {
            let new_onset = self.total_frames + (peak * self.hop_size as f32).round() as usize;
            if self.last_onset + self.min_ioi < new_onset
                && !(self.last_onset > 0 && self.delay > new_onset)
            {
                let delayed_onset = self.delay.max(new_onset);
                let estimated = delayed_onset.saturating_sub(self.delay) as u64;
                if accepts(estimated) {
                    self.last_onset = delayed_onset;
                    accepted = Some((estimated, new_onset as u64));
                }
            }
        } else if peak <= 0.0 && self.total_frames <= self.delay && !silent {
            let new_onset = self.total_frames;
            if self.total_frames == 0 || self.last_onset + self.min_ioi < new_onset {
                let estimated = new_onset as u64;
                if accepts(estimated) {
                    self.last_onset = self.total_frames + self.delay;
                    accepted = Some((estimated, new_onset as u64));
                }
            }
        }

        self.total_frames += self.hop_size;
        accepted
    }
}

struct PhaseVocoder {
    window_size: usize,
    hop_size: usize,
    old_size: usize,
    old: Vec<f32>,
    frame: Vec<f32>,
    fft_buffer: Vec<Complex32>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

struct Spectrum {
    magnitudes: Vec<f32>,
    phases: Vec<f32>,
}

impl PhaseVocoder {
    fn new(window_size: usize, hop_size: usize) -> Self {
        let mut planner = FftPlanner::new();
        let fft = planner.plan_fft_forward(window_size);
        let window = (0..window_size)
            .map(|i| 0.5 * (1.0 - (std::f32::consts::TAU * i as f32 / window_size as f32).cos()))
            .collect();
        let old_size = window_size.saturating_sub(hop_size);

        Self {
            window_size,
            hop_size,
            old_size,
            old: vec![0.0; old_size.max(1)],
            frame: vec![0.0; window_size],
            fft_buffer: vec![Complex32::new(0.0, 0.0); window_size],
            window,
            fft,
        }
    }

    fn spectrum(&mut self, input: &[f32]) -> Spectrum {
        debug_assert_eq!(input.len(), self.hop_size);

        if self.old_size > 0 {
            self.frame[..self.old_size].copy_from_slice(&self.old[..self.old_size]);
        }
        self.frame[self.old_size..self.old_size + self.hop_size].copy_from_slice(input);
        if self.old_size > 0 {
            self.old[..self.old_size]
                .copy_from_slice(&self.frame[self.hop_size..self.hop_size + self.old_size]);
        }

        for (index, fft_sample) in self.fft_buffer.iter_mut().enumerate() {
            let shifted_index = (index + self.window_size / 2) % self.window_size;
            *fft_sample =
                Complex32::new(self.frame[shifted_index] * self.window[shifted_index], 0.0);
        }
        self.fft.process(&mut self.fft_buffer);

        let bins = &self.fft_buffer[..=self.window_size / 2];
        let magnitudes = bins.iter().map(|bin| bin.norm()).collect();
        let mut phases: Vec<f32> = bins.iter().map(|bin| bin.arg()).collect();
        // Aubio forces the real-only DC and Nyquist bins to exactly 0 or PI.
        // Preserving tiny backend imaginary noise here creates false complex-
        // domain novelty in otherwise stable decays.
        if let Some(first) = bins.first() {
            phases[0] = if first.re < 0.0 {
                std::f32::consts::PI
            } else {
                0.0
            };
        }
        if let Some(last) = bins.last() {
            let last_index = phases.len() - 1;
            phases[last_index] = if last.re < 0.0 {
                std::f32::consts::PI
            } else {
                0.0
            };
        }
        Spectrum { magnitudes, phases }
    }
}

/// Aubio 0.4.9's complex-domain spectral descriptor, including the adaptive
/// whitening and log-compression defaults used by its `complex` onset mode.
struct ComplexDomain {
    previous_magnitudes: Vec<f32>,
    previous_phases: Vec<f32>,
    phases_two_frames_back: Vec<f32>,
    whitening_peaks: Vec<f32>,
    whitening_decay: f32,
}

impl ComplexDomain {
    const WHITENING_FLOOR: f32 = 1.0e-4;
    const WHITENING_RELAX_SECONDS: f32 = 250.0;
    const WHITENING_DECAY: f32 = 0.001;

    fn new(bins: usize, sample_rate: u32, hop_size: usize) -> Self {
        let whitening_decay = Self::WHITENING_DECAY
            .powf((hop_size as f32 / sample_rate as f32) / Self::WHITENING_RELAX_SECONDS);
        Self {
            previous_magnitudes: vec![0.0; bins],
            previous_phases: vec![0.0; bins],
            phases_two_frames_back: vec![0.0; bins],
            whitening_peaks: vec![Self::WHITENING_FLOOR; bins],
            whitening_decay,
        }
    }

    fn process(&mut self, spectrum: &Spectrum) -> f32 {
        let mut novelty = 0.0f32;
        for index in 0..spectrum.magnitudes.len() {
            let decayed_peak =
                (self.whitening_decay * self.whitening_peaks[index]).max(Self::WHITENING_FLOOR);
            self.whitening_peaks[index] = spectrum.magnitudes[index].max(decayed_peak);
            let whitened = spectrum.magnitudes[index] / self.whitening_peaks[index];
            let magnitude = whitened.ln_1p();
            let predicted_phase =
                2.0 * self.previous_phases[index] - self.phases_two_frames_back[index];
            let squared_distance = self.previous_magnitudes[index].powi(2) + magnitude.powi(2)
                - 2.0
                    * self.previous_magnitudes[index]
                    * magnitude
                    * (predicted_phase - spectrum.phases[index]).cos();
            novelty += squared_distance.abs().sqrt();

            self.phases_two_frames_back[index] = self.previous_phases[index];
            self.previous_phases[index] = spectrum.phases[index];
            self.previous_magnitudes[index] = magnitude;
        }
        novelty
    }
}

struct PeakPicker {
    threshold: f32,
    novelty: [f32; 7],
    peaks: [f32; 3],
}

impl PeakPicker {
    fn new(threshold: f32) -> Self {
        Self {
            threshold,
            novelty: [0.0; 7],
            peaks: [0.0; 3],
        }
    }

    fn process(&mut self, descriptor: f32) -> f32 {
        self.novelty.rotate_left(1);
        self.novelty[6] = descriptor;

        let mut filtered = self.novelty;
        filter_forward_backward(&mut filtered);
        let mean = filtered.iter().sum::<f32>() / filtered.len() as f32;
        let mut ordered = filtered;
        ordered.sort_by(f32::total_cmp);
        let median = ordered[ordered.len() / 2];
        let thresholded = filtered[5] - median - mean * self.threshold;

        self.peaks.rotate_left(1);
        self.peaks[2] = thresholded;
        if self.peaks[1] > self.peaks[0] && self.peaks[1] > self.peaks[2] && self.peaks[1] > 0.0 {
            quadratic_peak_position(self.peaks)
        } else {
            0.0
        }
    }
}

fn filter_forward_backward(values: &mut [f32]) {
    filter(values);
    values.reverse();
    filter(values);
    values.reverse();
}

fn filter(values: &mut [f32]) {
    const B: [f64; 3] = [0.159_987_89, 0.319_975_77, 0.159_987_89];
    const A: [f64; 3] = [1.0, 0.234_840_48, 0.0];
    let mut x = [0.0f64; 3];
    let mut y = [0.0f64; 3];

    for value in values {
        x[0] = *value as f64;
        y[0] = B[0] * x[0] + B[1] * x[1] + B[2] * x[2] - A[1] * y[1] - A[2] * y[2];
        *value = y[0] as f32;
        x[2] = x[1];
        x[1] = x[0];
        y[2] = y[1];
        y[1] = y[0];
    }
}

fn quadratic_peak_position(values: [f32; 3]) -> f32 {
    let denominator = values[0] - 2.0 * values[1] + values[2];
    if denominator.abs() <= f32::EPSILON {
        1.0
    } else {
        1.0 + 0.5 * (values[0] - values[2]) / denominator
    }
}

fn db_spl(input: &[f32]) -> f32 {
    if input.is_empty() {
        return f32::NEG_INFINITY;
    }
    let mean_square = input.iter().map(|sample| sample * sample).sum::<f32>() / input.len() as f32;
    10.0 * mean_square.log10()
}

#[cfg(test)]
mod tests {
    use super::*;

    // These validation cases port test_wrong_params from aubio 0.4.9's
    // tests/src/onset/test-onset.c.
    #[test]
    fn ported_upstream_constructor_validation() {
        let base = Config::default();
        assert_eq!(
            Config {
                hop_size: 0,
                ..base
            }
            .validate(44_100),
            Err(ConfigError::ZeroHopSize)
        );
        assert_eq!(
            Config {
                window_size: 1,
                hop_size: 1,
                ..base
            }
            .validate(44_100),
            Err(ConfigError::WindowTooSmall)
        );
        assert_eq!(
            Config {
                window_size: 256,
                hop_size: 512,
                ..base
            }
            .validate(44_100),
            Err(ConfigError::HopLargerThanWindow)
        );
        assert_eq!(base.validate(0), Err(ConfigError::ZeroSampleRate));
    }

    // Port of tests/src/onset/test-peakpicker.c's zero-input smoke case,
    // strengthened to assert that no false peak is emitted.
    #[test]
    fn ported_upstream_peak_picker_zero_input_stays_quiet() {
        let mut picker = PeakPicker::new(0.3);
        for _ in 0..4 {
            assert_eq!(picker.process(0.0), 0.0);
        }
    }

    // Port of the complex-domain zero-spectrum case in aubio 0.4.9's
    // tests/src/spectral/test-specdesc.c, strengthened to check repeated
    // frames because the descriptor retains two frames of phase history.
    #[test]
    fn ported_upstream_complex_zero_spectrum_is_zero() {
        let mut descriptor = ComplexDomain::new(513, 44_100, 256);
        let spectrum = Spectrum {
            magnitudes: vec![0.0; 513],
            phases: vec![0.0; 513],
        };
        assert_eq!(descriptor.process(&spectrum), 0.0);
        assert_eq!(descriptor.process(&spectrum), 0.0);
        assert_eq!(descriptor.process(&spectrum), 0.0);
    }

    #[test]
    fn sensitivity_never_changes_the_minimum_hit_spacing() {
        let fewer = Config::for_complex_candidates(44_100, TransientSensitivity::new(0));
        let balanced = Config::for_complex_candidates(44_100, TransientSensitivity::new(50));
        let more = Config::for_complex_candidates(44_100, TransientSensitivity::new(100));
        assert_eq!(more.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert_eq!(balanced.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert_eq!(fewer.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert!(more.threshold < balanced.threshold);
        assert!(balanced.threshold < fewer.threshold);

        let candidates = Config::for_complex_candidates(44_100, TransientSensitivity::DEFAULT);
        assert_eq!(candidates.threshold, COMPLEX_DEFAULT_THRESHOLD);
        assert_eq!(candidates.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert_eq!(candidates.delay_hops, COMPLEX_DELAY_HOPS);
        assert_eq!(
            Config::for_complex_candidates(48_000, TransientSensitivity::DEFAULT).window_size,
            1_024
        );
        assert_eq!(
            Config::for_complex_candidates(96_000, TransientSensitivity::DEFAULT).window_size,
            2_048
        );
    }

    #[test]
    fn silence_has_no_onsets() {
        assert!(
            detect_complex_onsets_where(&vec![0.0; 22_050], 44_100, Config::default(), |_| true)
                .is_empty()
        );
    }

    #[test]
    fn complex_primary_attacks_match_aubio_0_4_9_golden_frames() {
        let sample_rate = 44_100u32;
        let mut audio = vec![0.0f32; sample_rate as usize];
        for start in [2_000usize, 13_000, 24_000, 35_000] {
            for offset in 0..7_000 {
                if start + offset >= audio.len() {
                    break;
                }
                let time = offset as f32 / sample_rate as f32;
                let decay = (-(offset as f32) / 2_500.0).exp();
                audio[start + offset] += decay
                    * ((std::f32::consts::TAU * 120.0 * time).sin()
                        + (std::f32::consts::TAU * 128.0 * time).sin())
                    * 0.4;
            }
        }

        // Generated with aubio 0.4.9's complex mode and its upstream 0.15
        // threshold. RustFFT can produce additional low-novelty candidates in
        // a beating decay, which the producer-facing attack validator rejects,
        // but each upstream primary event must retain its timestamp.
        let upstream = [1_459i64, 12_710, 23_714, 34_717];
        let actual = detect_complex_onsets_where(
            &audio,
            sample_rate,
            Config {
                threshold: 0.15,
                delay_hops: COMPLEX_DELAY_HOPS,
                ..Config::default()
            },
            |_| true,
        );
        for expected in upstream {
            assert!(
                actual
                    .iter()
                    .any(|(actual, _)| (*actual as i64 - expected).abs() <= 3),
                "expected {expected}; all={actual:?}"
            );
        }
    }
}
