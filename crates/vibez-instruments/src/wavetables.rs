//! The Wavetable Synth's tables: each a run of single-cycle frames that
//! Position morphs through, kept at a level per octave so each holds only
//! the harmonics that stay under Nyquist where it is played.
//!
//! Every frame is drawn as one cycle at high resolution, taken apart by FFT
//! and rebuilt at each level with the harmonics above that level's limit
//! left out: the mipmapped, band-limited approach of the wavetable chapter
//! in J. Pekonen and V. Valimaki, "The brief history of virtual analog
//! synthesis", Forum Acusticum 2011, and of T. Stilson and J. O. Smith,
//! "Alias-Free Digital Synthesis of Classic Analog Waveforms", ICMC 1996.
//! Formant frames weigh the harmonics by vowel formants after G. E.
//! Peterson and H. L. Barney, "Control Methods Used in a Study of the
//! Vowels", JASA 24(2), 1952.

use std::sync::OnceLock;

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

/// Samples in one cycle of a frame.
pub const TABLE_SIZE: usize = 1024;
/// Frames Position morphs through.
pub const FRAMES: usize = 64;
/// Band-limited copies of each frame, the first with every harmonic a
/// cycle of [`TABLE_SIZE`] holds and each next with half as many.
pub const LEVELS: usize = 8;
const HARMONICS: usize = TABLE_SIZE / 2;
/// The resolution a frame is drawn at before it is taken apart.
const DRAWN: usize = TABLE_SIZE * 8;

pub mod table {
    pub const BASIC_SHAPES: usize = 0;
    pub const HARMONIC: usize = 1;
    pub const FORMANT: usize = 2;
    pub const PULSE: usize = 3;
    pub const SYNC: usize = 4;
    pub const COUNT: usize = 5;
}

/// One table: every frame at every level, one cycle each.
pub struct Wavetable {
    samples: Vec<f32>,
}

impl Wavetable {
    fn cycle(&self, frame: usize, level: usize) -> &[f32] {
        let start = (frame * LEVELS + level) * TABLE_SIZE;
        &self.samples[start..start + TABLE_SIZE]
    }

    /// A frame with every harmonic, for drawing.
    pub fn frame(&self, frame: usize) -> &[f32] {
        self.cycle(frame.min(FRAMES - 1), 0)
    }

    /// The table at `position` through its frames (0 to 1) and `phase`
    /// through the cycle (0 to 1), from `level`.
    pub fn read(&self, position: f32, phase: f32, level: usize) -> f32 {
        let place = position.clamp(0.0, 1.0) * (FRAMES - 1) as f32;
        let first = (place as usize).min(FRAMES - 2);
        let between = place - first as f32;
        let level = level.min(LEVELS - 1);
        let at = phase.rem_euclid(1.0) * TABLE_SIZE as f32;
        let index = (at as usize).min(TABLE_SIZE - 1);
        let fraction = at - index as f32;
        let next = (index + 1) % TABLE_SIZE;
        let sample = |cycle: &[f32]| cycle[index] + fraction * (cycle[next] - cycle[index]);
        let a = sample(self.cycle(first, level));
        if between <= 0.0 {
            return a;
        }
        a + between * (sample(self.cycle(first + 1, level)) - a)
    }
}

/// The level whose harmonics all stay under Nyquist for a cycle advancing
/// `increment` of itself a sample.
pub fn level_for(increment: f64) -> usize {
    let allowed = 0.5 / increment.abs().max(1e-12);
    (0..LEVELS)
        .find(|&level| (HARMONICS >> level) as f64 <= allowed)
        .unwrap_or(LEVELS - 1)
}

/// Every table, built once on first use.
pub fn tables() -> &'static [Wavetable; table::COUNT] {
    static TABLES: OnceLock<[Wavetable; table::COUNT]> = OnceLock::new();
    TABLES.get_or_init(|| std::array::from_fn(build))
}

fn build(table: usize) -> Wavetable {
    let mut planner = FftPlanner::<f32>::new();
    let forward = planner.plan_fft_forward(DRAWN);
    let inverse = planner.plan_fft_inverse(TABLE_SIZE);
    let mut samples = vec![0.0; FRAMES * LEVELS * TABLE_SIZE];
    for frame in 0..FRAMES {
        let t = frame as f32 / (FRAMES - 1) as f32;
        let spectrum = match harmonics(table, t) {
            Some(amplitudes) => {
                // A sine of amplitude a puts -ia N/2 in its bin and the
                // conjugate in the mirror bin.
                let mut spectrum = vec![Complex::new(0.0, 0.0); DRAWN];
                for (k, amplitude) in amplitudes.iter().enumerate().skip(1) {
                    spectrum[k] = Complex::new(0.0, -amplitude * DRAWN as f32 / 2.0);
                    spectrum[DRAWN - k] = Complex::new(0.0, amplitude * DRAWN as f32 / 2.0);
                }
                spectrum
            }
            None => {
                let mut spectrum: Vec<Complex<f32>> = draw(table, t)
                    .into_iter()
                    .map(|sample| Complex::new(sample, 0.0))
                    .collect();
                forward.process(&mut spectrum);
                spectrum
            }
        };
        let mut peak = 0.0f32;
        for level in 0..LEVELS {
            let limit = HARMONICS >> level;
            let mut cycle = vec![Complex::new(0.0, 0.0); TABLE_SIZE];
            for harmonic in 1..=limit.min(HARMONICS - 1) {
                cycle[harmonic] = spectrum[harmonic];
                cycle[TABLE_SIZE - harmonic] = spectrum[DRAWN - harmonic];
            }
            inverse.process(&mut cycle);
            let start = (frame * LEVELS + level) * TABLE_SIZE;
            for (sample, value) in samples[start..start + TABLE_SIZE].iter_mut().zip(&cycle) {
                *sample = value.re / DRAWN as f32;
            }
            if level == 0 {
                peak = samples[start..start + TABLE_SIZE]
                    .iter()
                    .fold(0.0f32, |peak, sample| peak.max(sample.abs()));
            }
        }
        // Each frame peaks at one with all its harmonics, so Position
        // morphs timbre rather than level.
        let start = frame * LEVELS * TABLE_SIZE;
        let scale = if peak > 1e-9 { 1.0 / peak } else { 0.0 };
        for sample in &mut samples[start..start + LEVELS * TABLE_SIZE] {
            *sample *= scale;
        }
    }
    Wavetable { samples }
}

/// The sine amplitudes, by harmonic from the fundamental at 1, of a table
/// made of its harmonics; `None` for one drawn as a waveform.
fn harmonics(table: usize, t: f32) -> Option<Vec<f32>> {
    match table {
        table::HARMONIC => {
            // From the fundamental alone to a bright, even spread of partials.
            let partials = 1 + (t * 47.0).round() as usize;
            let tilt = 2.0 - 1.2 * t;
            Some(
                (0..=partials)
                    .map(|k| {
                        if k == 0 {
                            0.0
                        } else {
                            1.0 / (k as f32).powf(tilt)
                        }
                    })
                    .collect(),
            )
        }
        table::FORMANT => Some(std::iter::once(0.0).chain(formant_weights(t)).collect()),
        _ => None,
    }
}

/// One cycle of `table` at `t` through its frames, at the drawing
/// resolution.
fn draw(table: usize, t: f32) -> Vec<f32> {
    let phase = |n: usize| n as f32 / DRAWN as f32;
    let tau = std::f32::consts::TAU;
    match table {
        table::PULSE => {
            let width = 0.5 - 0.47 * t;
            (0..DRAWN)
                .map(|n| if phase(n) < width { 1.0 } else { -1.0 })
                .collect()
        }
        table::SYNC => {
            // A saw reset by a master cycle, its own speed rising.
            let ratio = 1.0 + 7.0 * t;
            (0..DRAWN)
                .map(|n| 2.0 * (phase(n) * ratio).fract() - 1.0)
                .collect()
        }
        _ => {
            // Sine to triangle to saw to square, a third of the way each.
            let shapes = |p: f32| {
                let sine = (tau * p).sin();
                let triangle = 4.0 * ((p - 0.25).rem_euclid(1.0) - 0.5).abs() - 1.0;
                let saw = 2.0 * (p + 0.5).fract() - 1.0;
                let square = if p < 0.5 { 1.0 } else { -1.0 };
                [sine, triangle, saw, square]
            };
            let place = t * 3.0;
            let from = (place as usize).min(2);
            let between = place - from as f32;
            (0..DRAWN)
                .map(|n| {
                    let shapes = shapes(phase(n));
                    shapes[from] + between * (shapes[from + 1] - shapes[from])
                })
                .collect()
        }
    }
}

/// The first three formants of A, E, I, O and U in Hz, from Peterson and
/// Barney's averages for men's voices.
const VOWELS: [[f32; 3]; 5] = [
    [730.0, 1090.0, 2440.0],
    [530.0, 1840.0, 2480.0],
    [270.0, 2290.0, 3010.0],
    [570.0, 840.0, 2410.0],
    [300.0, 870.0, 2240.0],
];
/// The pitch the formants are placed for; above or below it they move with
/// the note, as a wavetable's do.
const FORMANT_PITCH_HZ: f32 = 110.0;
const FORMANT_HARMONICS: usize = 64;

/// How loud each harmonic is for a vowel `t` of the way from A to U.
fn formant_weights(t: f32) -> [f32; FORMANT_HARMONICS] {
    let place = t * (VOWELS.len() - 1) as f32;
    let from = (place as usize).min(VOWELS.len() - 2);
    let between = place - from as f32;
    let formants: [f32; 3] = std::array::from_fn(|i| {
        VOWELS[from][i] + between * (VOWELS[from + 1][i] - VOWELS[from][i])
    });
    std::array::from_fn(|k| {
        let hz = (k + 1) as f32 * FORMANT_PITCH_HZ;
        let peaks: f32 = formants
            .iter()
            .zip([1.0, 0.6, 0.3])
            .map(|(centre, strength)| {
                let width = 0.12 * centre;
                strength * (-0.5 * ((hz - centre) / width).powi(2)).exp()
            })
            .sum();
        // A gentle tilt keeps the fundamental under the formants.
        (peaks + 0.15 / (k + 1) as f32) / (k + 1) as f32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The amplitude of harmonic `k` in one cycle.
    fn harmonic(cycle: &[f32], k: usize) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for (n, sample) in cycle.iter().enumerate() {
            let phase = std::f64::consts::TAU * (k * n) as f64 / cycle.len() as f64;
            re += *sample as f64 * phase.cos();
            im += *sample as f64 * phase.sin();
        }
        (2.0 * (re * re + im * im).sqrt() / cycle.len() as f64) as f32
    }

    #[test]
    fn basic_shapes_starts_on_a_sine_and_ends_on_a_square() {
        let basic = &tables()[table::BASIC_SHAPES];
        let first = basic.frame(0);
        for (n, sample) in first.iter().enumerate() {
            let sine = (std::f32::consts::TAU * n as f32 / TABLE_SIZE as f32).sin();
            assert!((sample - sine).abs() < 1e-3, "{n}: {sample} {sine}");
        }
        let last = basic.frame(FRAMES - 1);
        // A square has odd harmonics only, the third a third of the first.
        let (h1, h2, h3) = (harmonic(last, 1), harmonic(last, 2), harmonic(last, 3));
        assert!(
            h2 < 0.01 * h1 && (h3 / h1 - 1.0 / 3.0).abs() < 0.02,
            "{h1} {h2} {h3}"
        );
    }

    #[test]
    fn each_level_keeps_only_the_harmonics_it_may() {
        for (index, wavetable) in tables().iter().enumerate() {
            for level in [0, 3, LEVELS - 1] {
                let cycle = wavetable.cycle(FRAMES - 1, level);
                let limit = HARMONICS >> level;
                for k in [limit + 1, limit * 2 - 1]
                    .into_iter()
                    .filter(|&k| k < HARMONICS)
                {
                    assert!(
                        harmonic(cycle, k) < 1e-4,
                        "table {index} level {level} harmonic {k}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_frame_peaks_at_one() {
        for wavetable in tables() {
            for frame in [0, FRAMES / 2, FRAMES - 1] {
                let peak = wavetable
                    .frame(frame)
                    .iter()
                    .fold(0.0f32, |p, s| p.max(s.abs()));
                assert!((peak - 1.0).abs() < 1e-3, "{peak}");
            }
        }
    }

    #[test]
    fn the_level_follows_the_pitch() {
        let rate = 48_000.0;
        let at = |hz: f64| level_for(hz / rate);
        // Low notes keep every harmonic; each octave up drops a level.
        assert_eq!(at(40.0), 0);
        assert_eq!(at(440.0) + 1, at(880.0));
        let level = at(440.0);
        let highest = (HARMONICS >> level) as f64 * 440.0;
        assert!(highest <= rate / 2.0, "{highest}");
        assert!(
            highest * 2.0 > rate / 2.0,
            "the level is the richest that fits"
        );
    }

    #[test]
    fn position_morphs_smoothly_between_frames() {
        let basic = &tables()[table::BASIC_SHAPES];
        let step = 1.0 / (FRAMES - 1) as f32;
        let a = basic.read(step, 0.3, 0);
        let b = basic.read(2.0 * step, 0.3, 0);
        let between = basic.read(1.5 * step, 0.3, 0);
        assert!((between - (a + b) / 2.0).abs() < 1e-5);
    }

    #[test]
    fn formants_move_from_a_to_u() {
        let formant = &tables()[table::FORMANT];
        // A's first formant sits near the seventh harmonic of 110 Hz, U's
        // near the third.
        let a = formant.frame(0);
        let u = formant.frame(FRAMES - 1);
        assert!(harmonic(a, 7) > harmonic(a, 3), "A");
        assert!(harmonic(u, 3) > harmonic(u, 7), "U");
    }
}
