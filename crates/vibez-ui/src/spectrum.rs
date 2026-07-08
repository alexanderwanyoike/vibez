//! Spectrum analyser for the channel EQ display.
//!
//! The engine streams post-effects mono samples for the tapped track
//! through a lock-free ring; every UI tick the analyser ingests them
//! into a sliding window, runs a Hann-windowed FFT, and folds the
//! magnitudes into log-spaced display bins with fast-attack /
//! slow-release smoothing (the familiar analyser ballistics).

/// FFT window length (samples). Power of two for radix-2.
const FFT_SIZE: usize = 2048;

/// Number of log-spaced display bins between MIN_HZ and MAX_HZ.
pub const DISPLAY_BINS: usize = 96;

/// Display range, matching the EQ curve's frequency axis.
pub const MIN_HZ: f32 = 20.0;
pub const MAX_HZ: f32 = 20_000.0;

/// Floor of the level axis in dB (0 dB = full scale).
pub const FLOOR_DB: f32 = -66.0;

/// Release rate in dB per tick (60 fps → ~1.6 s full-scale fall).
const RELEASE_DB_PER_TICK: f32 = 0.66;

#[derive(Debug, Clone)]
pub struct SpectrumState {
    /// Sliding window of the most recent mono samples.
    window: Vec<f32>,
    /// Smoothed display bins in dB (FLOOR_DB..0), ready to draw.
    pub bins: Vec<f32>,
    /// Anything above the floor to draw at all?
    pub active: bool,
}

impl Default for SpectrumState {
    fn default() -> Self {
        Self {
            window: Vec::with_capacity(FFT_SIZE),
            bins: vec![FLOOR_DB; DISPLAY_BINS],
            active: false,
        }
    }
}

impl SpectrumState {
    /// Append freshly drained tap samples, keeping the last window.
    pub fn ingest(&mut self, samples: &[f32]) {
        self.window.extend_from_slice(samples);
        let len = self.window.len();
        if len > FFT_SIZE {
            self.window.drain(..len - FFT_SIZE);
        }
    }

    /// Drop all signal history (tap retargeted to another track).
    pub fn reset(&mut self) {
        self.window.clear();
        self.bins.fill(FLOOR_DB);
        self.active = false;
    }

    /// One tick of analysis: FFT the current window and update the
    /// smoothed display bins.
    pub fn analyse(&mut self, sample_rate: f32) {
        let fresh = if self.window.len() >= FFT_SIZE {
            compute_bins(&self.window[self.window.len() - FFT_SIZE..], sample_rate)
        } else {
            // No (or not enough) signal: release toward the floor.
            vec![FLOOR_DB; DISPLAY_BINS]
        };
        let mut any = false;
        for (cur, new) in self.bins.iter_mut().zip(fresh) {
            if new > *cur {
                *cur = new; // fast attack
            } else {
                *cur = (*cur - RELEASE_DB_PER_TICK).max(new).max(FLOOR_DB);
            }
            if *cur > FLOOR_DB + 0.5 {
                any = true;
            }
        }
        self.active = any;
    }
}

/// Hann-window `input` (FFT_SIZE samples), FFT it, and fold the
/// magnitude spectrum into DISPLAY_BINS log-spaced dB bins.
fn compute_bins(input: &[f32], sample_rate: f32) -> Vec<f32> {
    debug_assert_eq!(input.len(), FFT_SIZE);
    let mut re: Vec<f32> = input
        .iter()
        .enumerate()
        .map(|(i, &s)| {
            let hann = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (FFT_SIZE - 1) as f32).cos();
            s * hann
        })
        .collect();
    let mut im = vec![0.0f32; FFT_SIZE];
    fft_in_place(&mut re, &mut im);

    // Coherent-gain normalization: a full-scale sine under a Hann
    // window peaks at N/4 in the half-spectrum.
    let norm = 4.0 / FFT_SIZE as f32;
    let hz_per_fft_bin = sample_rate / FFT_SIZE as f32;
    let log_min = MIN_HZ.ln();
    let log_span = MAX_HZ.ln() - log_min;

    let mut bins = vec![FLOOR_DB; DISPLAY_BINS];
    for k in 1..FFT_SIZE / 2 {
        let freq = k as f32 * hz_per_fft_bin;
        if !(MIN_HZ..=MAX_HZ).contains(&freq) {
            continue;
        }
        let pos = (freq.ln() - log_min) / log_span;
        let bin = ((pos * DISPLAY_BINS as f32) as usize).min(DISPLAY_BINS - 1);
        let mag = (re[k] * re[k] + im[k] * im[k]).sqrt() * norm;
        let db = (20.0 * mag.max(1e-9).log10()).clamp(FLOOR_DB, 0.0);
        if db > bins[bin] {
            bins[bin] = db;
        }
    }
    // Low display bins can be narrower than one FFT bin; fill gaps
    // from the left neighbor so the trace stays continuous.
    for i in 1..DISPLAY_BINS {
        if bins[i] <= FLOOR_DB && bins[i - 1] > FLOOR_DB {
            bins[i] = bins[i - 1] - 1.5;
        }
    }
    bins
}

/// Iterative radix-2 Cooley-Tukey FFT, in place. `re.len()` must be a
/// power of two and equal to `im.len()`.
fn fft_in_place(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two() && n == im.len());

    // Bit-reversal permutation.
    let bits = n.trailing_zeros();
    for i in 0..n {
        let j = (i as u32).reverse_bits() >> (32 - bits);
        let j = j as usize;
        if j > i {
            re.swap(i, j);
            im.swap(i, j);
        }
    }

    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (w_re, w_im) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cur_re, mut cur_im) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let t_re = re[b] * cur_re - im[b] * cur_im;
                let t_im = re[b] * cur_im + im[b] * cur_re;
                re[b] = re[a] - t_re;
                im[b] = im[a] - t_im;
                re[a] += t_re;
                im[a] += t_im;
                let next_re = cur_re * w_re - cur_im * w_im;
                cur_im = cur_re * w_im + cur_im * w_re;
                cur_re = next_re;
            }
        }
        len <<= 1;
    }
}

/// Normalized x position (0..1) of `freq` on the shared log axis.
pub fn freq_to_norm(freq: f32) -> f32 {
    ((freq.max(MIN_HZ).ln() - MIN_HZ.ln()) / (MAX_HZ.ln() - MIN_HZ.ln())).clamp(0.0, 1.0)
}

/// Inverse of [`freq_to_norm`].
pub fn norm_to_freq(pos: f32) -> f32 {
    (MIN_HZ.ln() + pos.clamp(0.0, 1.0) * (MAX_HZ.ln() - MIN_HZ.ln())).exp()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_finds_a_sine_peak() {
        let sr = 48_000.0f32;
        let freq = 1_000.0f32;
        let samples: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (std::f32::consts::TAU * freq * i as f32 / sr).sin() * 0.8)
            .collect();
        let bins = compute_bins(&samples, sr);

        // The display bin holding 1 kHz should carry the peak level.
        let expect_bin =
            ((freq_to_norm(freq) * DISPLAY_BINS as f32) as usize).min(DISPLAY_BINS - 1);
        let peak = bins
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .unwrap();
        assert!(
            (peak.0 as i32 - expect_bin as i32).abs() <= 1,
            "peak bin {} should sit at ~{expect_bin}",
            peak.0
        );
        // 0.8 full scale ≈ -1.9 dB.
        assert!(
            (*peak.1 - (-1.9)).abs() < 1.5,
            "peak level should read near -1.9 dB, got {}",
            peak.1
        );
        // Far-away bins stay near the floor (100 Hz vs 1 kHz).
        let far = bins[(freq_to_norm(100.0) * DISPLAY_BINS as f32) as usize];
        assert!(far < -40.0, "off-peak bin should be quiet, got {far}");
    }

    #[test]
    fn attack_is_instant_release_is_gradual() {
        let sr = 48_000.0f32;
        let mut state = SpectrumState::default();
        let tone: Vec<f32> = (0..FFT_SIZE)
            .map(|i| (std::f32::consts::TAU * 1_000.0 * i as f32 / sr).sin())
            .collect();
        state.ingest(&tone);
        state.analyse(sr);
        let peak_after_attack = state.bins.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(peak_after_attack > -3.0, "attack should be instant");
        assert!(state.active);

        // Silence: the peak falls by the release rate per tick, not
        // all at once.
        state.window.clear();
        state.analyse(sr);
        let peak_after_release = state.bins.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            (peak_after_attack - peak_after_release - RELEASE_DB_PER_TICK).abs() < 1e-3,
            "release should fall by exactly one tick's worth"
        );
    }

    #[test]
    fn freq_axis_roundtrips() {
        for f in [20.0, 100.0, 1_000.0, 10_000.0, 20_000.0] {
            let back = norm_to_freq(freq_to_norm(f));
            assert!((back / f - 1.0).abs() < 1e-3, "{f} -> {back}");
        }
    }
}
