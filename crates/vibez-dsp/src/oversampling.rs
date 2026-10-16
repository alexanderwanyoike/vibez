//! Doubling and halving a signal's rate with halfband FIR filters, for
//! nonlinear stages that would otherwise alias.
//!
//! A halfband lowpass has every other tap zero apart from the centre, so
//! each rate change costs only its non-zero taps. The taps are a windowed
//! sinc with a Kaiser window, the method in Oppenheim and Schafer,
//! "Discrete-Time Signal Processing", section 7.6.

/// `K` taps either side of the centre, odd so the centre and the `T = K + 1`
/// sinc taps fall on opposite sample phases.
#[derive(Clone)]
struct Halfband<const K: usize, const T: usize> {
    /// The non-zero taps other than the centre, which is one half, from
    /// the one applied to the oldest sample.
    reversed: [f32; T],
}

impl<const K: usize, const T: usize> Halfband<K, T> {
    fn new(beta: f64) -> Self {
        assert!(K % 2 == 1 && T == K + 1 && 2 * T <= HISTORY);
        let bessel = |x: f64| {
            let (mut sum, mut term) = (1.0, 1.0);
            for order in 1..64 {
                term *= x * x / (4.0 * (order * order) as f64);
                sum += term;
                if term <= sum * f64::EPSILON {
                    break;
                }
            }
            sum
        };
        let half = K as f64;
        let mut taps: [f64; T] = std::array::from_fn(|j| {
            let offset = (2 * j) as f64 - half;
            let sinc =
                (std::f64::consts::FRAC_PI_2 * offset).sin() / (std::f64::consts::PI * offset);
            let window =
                bessel(beta * (1.0 - (offset / half).powi(2)).max(0.0).sqrt()) / bessel(beta);
            sinc * window
        });
        // With the centre at one half, the sinc taps make up the other half
        // of unity gain at DC.
        let sum: f64 = taps.iter().sum();
        taps.iter_mut().for_each(|tap| *tap *= 0.5 / sum);
        taps.reverse();
        Self {
            reversed: taps.map(|tap| tap as f32),
        }
    }

    /// The taps against the newest `T` samples of `history`, oldest first.
    fn dot(&self, history: &[f32]) -> f32 {
        self.reversed
            .iter()
            .zip(history)
            .map(|(tap, sample)| tap * sample)
            .sum()
    }
}

/// Room for a history written twice over, so the newest `T` samples are
/// always one contiguous slice.
const HISTORY: usize = 32;

#[derive(Clone)]
struct History<const T: usize> {
    samples: [f32; HISTORY],
}

impl<const T: usize> History<T> {
    const EMPTY: Self = Self {
        samples: [0.0; HISTORY],
    };

    fn write(&mut self, position: usize, sample: f32) {
        self.samples[position] = sample;
        self.samples[position + T] = sample;
    }

    /// The `T` samples up to the one at `position`, oldest first.
    fn recent(&self, position: usize) -> &[f32] {
        &self.samples[position + 1..position + T + 1]
    }

    /// The sample `age` before the one at `position`.
    fn before(&self, position: usize, age: usize) -> f32 {
        self.samples[position + T - age]
    }
}

/// One rate doubling and halving for up to two channels, delaying the
/// round trip by `K` samples at the source rate.
#[derive(Clone)]
pub struct Stage<const K: usize, const T: usize> {
    filter: Halfband<K, T>,
    up: [History<T>; 2],
    down_even: [History<T>; 2],
    down_odd: [History<T>; 2],
    position: [usize; 2],
}

impl<const K: usize, const T: usize> Stage<K, T> {
    pub fn new(beta: f64) -> Self {
        Self {
            filter: Halfband::new(beta),
            up: [History::EMPTY, History::EMPTY],
            down_even: [History::EMPTY, History::EMPTY],
            down_odd: [History::EMPTY, History::EMPTY],
            position: [0; 2],
        }
    }

    /// The two samples at twice the rate that follow `input`. Call
    /// [`Self::down`] next, which moves the stage on a sample.
    pub fn up(&mut self, channel: usize, input: f32) -> [f32; 2] {
        let newest = self.position[channel];
        let history = &mut self.up[channel];
        history.write(newest, input);
        let interpolated = 2.0 * self.filter.dot(history.recent(newest));
        [interpolated, history.before(newest, (K - 1) / 2)]
    }

    /// One sample at the source rate from the pair at twice the rate.
    pub fn down(&mut self, channel: usize, pair: [f32; 2]) -> f32 {
        let newest = self.position[channel];
        self.down_even[channel].write(newest, pair[0]);
        self.down_odd[channel].write(newest, pair[1]);
        let centre = self.down_odd[channel].before(newest, K.div_ceil(2));
        let output = self.filter.dot(self.down_even[channel].recent(newest)) + 0.5 * centre;
        self.position[channel] = (newest + 1) % T;
        output
    }

    pub fn reset(&mut self) {
        self.up = [History::EMPTY, History::EMPTY];
        self.down_even = [History::EMPTY, History::EMPTY];
        self.down_odd = [History::EMPTY, History::EMPTY];
        self.position = [0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_round_trip_returns_the_band_unchanged_k_samples_later() {
        let mut stage = Stage::<15, 16>::new(7.0);
        let input: Vec<f32> = (0..2000)
            .map(|n| (std::f32::consts::TAU * 0.05 * n as f32).sin())
            .collect();
        let output: Vec<f32> = input
            .iter()
            .map(|&sample| {
                let pair = stage.up(0, sample);
                stage.down(0, pair)
            })
            .collect();
        for (got, want) in output[15..].iter().zip(&input).skip(100) {
            assert!((got - want).abs() < 1e-3, "{got} vs {want}");
        }
    }

    #[test]
    fn doubling_keeps_every_original_sample_between_new_ones() {
        let mut stage = Stage::<7, 8>::new(6.0);
        let mut originals = Vec::new();
        for n in 0..64 {
            let pair = stage.up(0, n as f32);
            originals.push(pair[1]);
            stage.down(0, pair);
        }
        for (n, original) in originals.iter().enumerate().skip(3) {
            assert_eq!(*original, (n - 3) as f32);
        }
    }
}
