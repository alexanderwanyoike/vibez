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
const DEFAULT_THRESHOLD: f32 = 0.058;
const DEFAULT_ENERGY_THRESHOLD: f32 = 0.3;
const DEFAULT_MIN_IOI_MS: f32 = 50.0;
const DEFAULT_SILENCE_DB: f32 = -70.0;
const DEFAULT_DELAY_HOPS: f32 = 4.3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Config {
    window_size: usize,
    hop_size: usize,
    threshold: f32,
    min_ioi_ms: f32,
    silence_db: f32,
}

impl Config {
    pub(super) fn for_sensitivity(sensitivity: TransientSensitivity) -> Self {
        // aubio documents 0.001..=0.900 as the useful peak-threshold range.
        // Interpolate logarithmically through its HFC default at 50%, giving
        // useful travel at both ends instead of bunching every result near the
        // middle of the knob.
        Self {
            threshold: sensitivity.threshold_through(DEFAULT_THRESHOLD),
            ..Self::default()
        }
    }

    pub(super) fn for_energy_sensitivity(sensitivity: TransientSensitivity) -> Self {
        Self {
            threshold: sensitivity.threshold_through(DEFAULT_ENERGY_THRESHOLD),
            ..Self::default()
        }
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
            threshold: DEFAULT_THRESHOLD,
            min_ioi_ms: DEFAULT_MIN_IOI_MS,
            silence_db: DEFAULT_SILENCE_DB,
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

pub(super) fn detect_onsets(mono: &[f32], sample_rate: u32, config: Config) -> Vec<u64> {
    detect_onsets_with_descriptor(mono, sample_rate, config, Descriptor::Hfc)
}

pub(super) fn detect_energy_onsets(mono: &[f32], sample_rate: u32, config: Config) -> Vec<u64> {
    detect_onsets_with_descriptor(mono, sample_rate, config, Descriptor::Energy)
}

fn detect_onsets_with_descriptor(
    mono: &[f32],
    sample_rate: u32,
    config: Config,
    descriptor: Descriptor,
) -> Vec<u64> {
    let Ok(config) = config.validate(sample_rate) else {
        return Vec::new();
    };
    if mono.is_empty() {
        return Vec::new();
    }

    Detector::new(sample_rate, config, descriptor).process(mono)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Descriptor {
    Hfc,
    Energy,
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
    descriptor: Descriptor,
}

impl Detector {
    fn new(sample_rate: u32, config: Config, descriptor: Descriptor) -> Self {
        Self {
            hop_size: config.hop_size,
            min_ioi: ((config.min_ioi_ms / 1_000.0) * sample_rate as f32).round() as usize,
            delay: (DEFAULT_DELAY_HOPS * config.hop_size as f32) as usize,
            silence_db: config.silence_db,
            total_frames: 0,
            last_onset: 0,
            phase_vocoder: PhaseVocoder::new(config.window_size, config.hop_size),
            peak_picker: PeakPicker::new(config.threshold),
            descriptor,
        }
    }

    fn process(mut self, mono: &[f32]) -> Vec<u64> {
        let mut onsets = Vec::new();
        let mut hop = vec![0.0; self.hop_size];

        for chunk in mono.chunks(self.hop_size) {
            hop.fill(0.0);
            hop[..chunk.len()].copy_from_slice(chunk);
            if let Some(frame) = self.process_hop(&hop) {
                onsets.push(frame as u64);
            }
        }
        onsets
    }

    fn process_hop(&mut self, input: &[f32]) -> Option<usize> {
        let magnitudes = self.phase_vocoder.spectrum(input);
        let descriptor = match self.descriptor {
            Descriptor::Hfc => high_frequency_content(&magnitudes),
            Descriptor::Energy => spectral_energy(&magnitudes),
        };
        let peak = self.peak_picker.process(descriptor);
        let silent = db_spl(input) < self.silence_db;
        let mut accepted = false;

        if peak > 0.0 && !silent {
            let new_onset = self.total_frames + (peak * self.hop_size as f32).round() as usize;
            if self.last_onset + self.min_ioi < new_onset
                && !(self.last_onset > 0 && self.delay > new_onset)
            {
                self.last_onset = self.delay.max(new_onset);
                accepted = true;
            }
        } else if peak <= 0.0 && self.total_frames <= self.delay && !silent {
            let new_onset = self.total_frames;
            if self.total_frames == 0 || self.last_onset + self.min_ioi < new_onset {
                self.last_onset = self.total_frames + self.delay;
                accepted = true;
            }
        }

        self.total_frames += self.hop_size;
        accepted.then(|| self.last_onset.saturating_sub(self.delay))
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

    fn spectrum(&mut self, input: &[f32]) -> Vec<f32> {
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

        self.fft_buffer[..=self.window_size / 2]
            .iter()
            .map(|bin| bin.norm())
            .collect()
    }
}

fn spectral_energy(magnitudes: &[f32]) -> f32 {
    magnitudes
        .iter()
        .map(|magnitude| magnitude * magnitude)
        .sum()
}

fn high_frequency_content(magnitudes: &[f32]) -> f32 {
    magnitudes
        .iter()
        .enumerate()
        .map(|(index, magnitude)| (index + 1) as f32 * magnitude.ln_1p())
        .sum()
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

    // Port of the zero-spectrum cases in tests/src/spectral/test-specdesc.c.
    #[test]
    fn ported_upstream_hfc_zero_spectrum_is_zero() {
        assert_eq!(high_frequency_content(&vec![0.0; 513]), 0.0);
    }

    // Port of the energy zero-spectrum case in
    // tests/src/spectral/test-specdesc.c.
    #[test]
    fn ported_upstream_energy_zero_spectrum_is_zero() {
        assert_eq!(spectral_energy(&vec![0.0; 513]), 0.0);
    }

    #[test]
    fn sensitivity_never_changes_the_minimum_hit_spacing() {
        let fewer = Config::for_sensitivity(TransientSensitivity::new(0));
        let balanced = Config::for_sensitivity(TransientSensitivity::new(50));
        let more = Config::for_sensitivity(TransientSensitivity::new(100));
        assert_eq!(more.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert_eq!(balanced.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert_eq!(fewer.min_ioi_ms, DEFAULT_MIN_IOI_MS);
        assert!(more.threshold < balanced.threshold);
        assert!(balanced.threshold < fewer.threshold);
    }

    #[test]
    fn silence_has_no_onsets() {
        assert!(detect_onsets(&vec![0.0; 22_050], 44_100, Config::default()).is_empty());
    }

    #[test]
    fn constant_signal_matches_aubio_start_marker() {
        assert_eq!(
            detect_onsets(&vec![0.25; 44_100], 44_100, Config::default()),
            vec![0]
        );
    }

    #[test]
    fn decaying_bursts_match_aubio_0_4_9_golden_frames() {
        let mut audio = vec![0.0f32; 44_100];
        for start in [2_048usize, 8_192, 16_384, 28_672] {
            for offset in 0..4_096 {
                if start + offset >= audio.len() {
                    break;
                }
                let phase = std::f32::consts::TAU * 120.0 * offset as f32 / 44_100.0;
                audio[start + offset] += phase.sin() * (-(offset as f32) / 300.0).exp() * 0.8;
            }
        }

        // Generated by aubio 0.4.9 with method=default, win=1024,
        // hop=256, sample_rate=44100. A two-sample allowance covers the
        // expected FFT/backend floating-point difference.
        let upstream = [1_943i64, 8_087, 16_279, 28_567];
        let actual = detect_onsets(&audio, 44_100, Config::default());
        assert_eq!(actual.len(), upstream.len(), "actual={actual:?}");
        for (actual, expected) in actual.iter().zip(upstream) {
            assert!(
                (*actual as i64 - expected).abs() <= 2,
                "expected {expected}, got {actual}; all={actual:?}"
            );
        }
    }
}
