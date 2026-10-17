//! Running a memoryless curve at 1x, 2x or 4x the host rate through the
//! halfband stages in [`crate::oversampling`], with each rate delayed to the
//! 4x path's latency so a device's delay never changes with its quality, and
//! a fade for switching between choices without a click.

use crate::oversampling::Stage;

pub mod quality {
    pub const X1: usize = 0;
    pub const X2: usize = 1;
    pub const X4: usize = 2;
    pub const COUNT: usize = 3;
}

/// Halfband sizes: the first stage needs a narrow transition, the second,
/// working on a signal already band-limited to a quarter of its rate, does not.
pub(crate) const K1: usize = 15;
pub(crate) const T1: usize = K1 + 1;
pub(crate) const K2: usize = 7;
pub(crate) const T2: usize = K2 + 1;
pub(crate) const BETA1: f64 = 7.0;
pub(crate) const BETA2: f64 = 6.0;
/// The 4x path: the first stage's round trip, then the second's, made a
/// whole number of host samples by one extra sample at twice the rate.
pub const LATENCY_SAMPLES: u32 = (K1 + K2.div_ceil(2)) as u32;
pub(crate) const LATENCY: usize = LATENCY_SAMPLES as usize;
/// The 2x path is short of the 4x path's delay by this much.
pub(crate) const X2_PAD: usize = LATENCY - K1;
pub const RAMP_FRAMES: u32 = 512;
/// Weights for a choice, moving from one option to another over
/// [`RAMP_FRAMES`] so a switch never clicks.
#[derive(Clone, Copy)]
pub(crate) struct Fade<const N: usize> {
    pub(crate) weights: [f32; N],
    steps: [f32; N],
    remaining: u32,
}

impl<const N: usize> Fade<N> {
    pub(crate) fn at(choice: usize) -> Self {
        Self {
            weights: std::array::from_fn(|index| f32::from(index == choice)),
            steps: [0.0; N],
            remaining: 0,
        }
    }

    pub(crate) fn toward(&mut self, choice: usize) {
        self.steps = std::array::from_fn(|index| {
            (f32::from(index == choice) - self.weights[index]) / RAMP_FRAMES as f32
        });
        self.remaining = RAMP_FRAMES;
    }

    pub(crate) fn advance(&mut self) {
        if self.remaining == 0 {
            return;
        }
        self.remaining -= 1;
        if self.remaining == 0 {
            self.weights = self.weights.map(f32::round);
        } else {
            for (weight, step) in self.weights.iter_mut().zip(self.steps) {
                *weight += step;
            }
        }
    }
}

/// Stereo frames delayed by `N`.
#[derive(Clone, Copy)]
pub(crate) struct Delay<const N: usize> {
    frames: [[f32; 2]; N],
    position: usize,
}

impl<const N: usize> Delay<N> {
    pub(crate) const EMPTY: Self = Self {
        frames: [[0.0; 2]; N],
        position: 0,
    };

    pub(crate) fn push(&mut self, frame: [f32; 2]) -> [f32; 2] {
        let out = std::mem::replace(&mut self.frames[self.position], frame);
        self.position = (self.position + 1) % N;
        out
    }
}

/// The newer curves at one rate, with that rate's filter state.
pub(crate) struct Path {
    first: Stage<K1, T1>,
    second: Stage<K2, T2>,
    /// The second stage's extra sample at twice the host rate.
    held: [f32; 2],
}

impl Path {
    pub(crate) fn new() -> Self {
        Self {
            first: Stage::new(BETA1),
            second: Stage::new(BETA2),
            held: [0.0; 2],
        }
    }

    pub(crate) fn reset(&mut self) {
        self.first.reset();
        self.second.reset();
        self.held = [0.0; 2];
    }
}

/// `curve` at each rate the `qualities` weigh in, for one channel's
/// `input`; a rate weighed at zero is left at zero, not computed.
pub(crate) fn rates(
    paths: &mut [Path; 2],
    channel: usize,
    input: f32,
    qualities: &[f32; quality::COUNT],
    curve: impl Fn(f32) -> f32,
) -> [f32; quality::COUNT] {
    let mut rates = [0.0; quality::COUNT];
    if qualities[quality::X1] > 0.0 {
        rates[quality::X1] = curve(input);
    }
    if qualities[quality::X2] > 0.0 {
        let stage = &mut paths[0].first;
        let pair = stage.up(channel, input).map(&curve);
        rates[quality::X2] = stage.down(channel, pair);
    }
    if qualities[quality::X4] > 0.0 {
        let path = &mut paths[1];
        let doubled = path.first.up(channel, input);
        let shaped = doubled.map(|sample| {
            let pair = path.second.up(channel, sample).map(&curve);
            path.second.down(channel, pair)
        });
        let held = std::mem::replace(&mut path.held[channel], shaped[1]);
        rates[quality::X4] = path.first.down(channel, [held, shaped[0]]);
    }
    rates
}
