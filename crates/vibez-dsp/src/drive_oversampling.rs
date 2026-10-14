//! Prepared polyphase FIR paths keep every Drive quality and character history warm.

const HOST_TAPS: usize = 65;
const MAX_TAPS: usize = 513;

pub(super) struct OversamplingPath {
    factor: usize,
    kernel: Vec<f32>,
    phases: Vec<[f32; HOST_TAPS]>,
    input: [[f32; HOST_TAPS * 2]; 2],
    input_position: [usize; 2],
    shaped: [[[f32; MAX_TAPS * 2]; 3]; 2],
    shaped_position: [usize; 2],
}

impl OversamplingPath {
    pub(super) fn new(factor: usize) -> Self {
        let taps = 64 * factor + 1;
        let midpoint = (taps - 1) as f64 * 0.5;
        let cutoff = 0.9 / factor as f64;
        let normalization = bessel_zero(8.6);
        let mut kernel: Vec<f64> = (0..taps)
            .map(|tap| {
                let offset = tap as f64 - midpoint;
                let argument = std::f64::consts::PI * cutoff * offset;
                let sinc = if argument == 0.0 {
                    1.0
                } else {
                    argument.sin() / argument
                };
                let window = bessel_zero(8.6 * (1.0 - (offset / midpoint).powi(2)).max(0.0).sqrt())
                    / normalization;
                cutoff * sinc * window
            })
            .collect();
        let sum: f64 = kernel.iter().sum();
        kernel.iter_mut().for_each(|value| *value /= sum);
        let phases = (0..factor)
            .map(|phase| {
                std::array::from_fn(|tap| {
                    kernel.get(phase + tap * factor).copied().unwrap_or(0.0) as f32 * factor as f32
                })
            })
            .collect();
        Self {
            factor,
            kernel: kernel.into_iter().map(|value| value as f32).collect(),
            phases,
            input: [[0.0; HOST_TAPS * 2]; 2],
            input_position: [0; 2],
            shaped: [[[0.0; MAX_TAPS * 2]; 3]; 2],
            shaped_position: [0; 2],
        }
    }

    pub(super) fn process(
        &mut self,
        input: f32,
        channel: usize,
        gain: f32,
        normalizer: f32,
        weights: [f32; 3],
    ) -> f32 {
        let head = self.input_position[channel];
        self.input[channel][head] = input;
        self.input[channel][head + HOST_TAPS] = input;
        let history = &self.input[channel][head + 1..head + HOST_TAPS + 1];
        let mut output = 0.0;
        for phase in 0..self.factor {
            let interpolated: f32 = if phase == 0 {
                symmetric_dot(history, &self.phases[phase])
            } else if phase * 2 == self.factor {
                symmetric_dot(&history[1..], &self.phases[phase][..HOST_TAPS - 1])
            } else {
                reversed_dot(history, &self.phases[phase])
            };
            let driven = interpolated * gain;
            let limited = driven.clamp(-1.0, 1.0);
            let curves = [
                driven.tanh() / normalizer,
                1.5 * limited - 0.5 * limited * limited * limited,
                limited,
            ];
            let position = self.shaped_position[channel];
            for curve in 0..3 {
                let samples = &mut self.shaped[channel][curve];
                samples[position] = curves[curve];
                samples[position + self.kernel.len()] = curves[curve];
                // Decimation at phase zero preserves the exact 64-host-frame
                // combined group delay; taking the last phase advances it.
                if phase == 0 && weights[curve] != 0.0 {
                    output += weights[curve]
                        * symmetric_dot(
                            &samples[position + 1..position + self.kernel.len() + 1],
                            &self.kernel,
                        );
                }
            }
            self.shaped_position[channel] = (position + 1) % self.kernel.len();
        }
        self.input_position[channel] = (head + 1) % HOST_TAPS;
        output
    }

    pub(super) fn reset(&mut self) {
        self.input = [[0.0; HOST_TAPS * 2]; 2];
        self.input_position = [0; 2];
        self.shaped = [[[0.0; MAX_TAPS * 2]; 3]; 2];
        self.shaped_position = [0; 2];
    }
}

fn bessel_zero(value: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for order in 1..64 {
        term *= value * value / (4.0 * (order * order) as f64);
        sum += term;
        if term <= sum * f64::EPSILON {
            break;
        }
    }
    sum
}

fn symmetric_dot(samples: &[f32], coefficients: &[f32]) -> f32 {
    let midpoint = coefficients.len() / 2;
    let mut sums = [0.0; 4];
    for group in 0..midpoint / 4 {
        for (lane, sum) in sums.iter_mut().enumerate() {
            let index = group * 4 + lane;
            *sum += (samples[index] + samples[samples.len() - 1 - index]) * coefficients[index];
        }
    }
    let mut result: f32 = sums.into_iter().sum();
    for index in midpoint / 4 * 4..midpoint {
        result += (samples[index] + samples[samples.len() - 1 - index]) * coefficients[index];
    }
    if coefficients.len() % 2 == 1 {
        result += samples[midpoint] * coefficients[midpoint];
    }
    if result.is_finite() {
        result
    } else {
        // Exceptional host headroom must not overflow intermediate f32 accumulators.
        super::finite_sample(
            samples
                .iter()
                .rev()
                .zip(coefficients)
                .map(|(&sample, &coefficient)| sample as f64 * coefficient as f64)
                .sum(),
        )
    }
}

fn reversed_dot(samples: &[f32], coefficients: &[f32]) -> f32 {
    let mut sums = [0.0; 4];
    for group in 0..coefficients.len() / 4 {
        for (lane, sum) in sums.iter_mut().enumerate() {
            let index = group * 4 + lane;
            *sum += samples[samples.len() - 1 - index] * coefficients[index];
        }
    }
    let mut result: f32 = sums.into_iter().sum();
    for index in coefficients.len() / 4 * 4..coefficients.len() {
        result += samples[samples.len() - 1 - index] * coefficients[index];
    }
    if result.is_finite() {
        result
    } else {
        // Exceptional host headroom must not overflow intermediate f32 accumulators.
        super::finite_sample(
            samples
                .iter()
                .rev()
                .zip(coefficients)
                .map(|(&sample, &coefficient)| sample as f64 * coefficient as f64)
                .sum(),
        )
    }
}
