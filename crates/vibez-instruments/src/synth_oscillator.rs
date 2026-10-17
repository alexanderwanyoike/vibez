//! Band-limited oscillators: the naive shapes with a polynomial correction
//! at each edge (PolyBLEP) or corner (PolyBLAMP), as in Välimäki and
//! Huovilainen, "Antialiasing Oscillators in Subtractive Synthesis" (2007),
//! and Tale's open-source PolyBLEP for REAPER's mono_synth.

/// The oscillator shapes, in their stored order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
}

impl Waveform {
    pub fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Saw,
            2 => Self::Square,
            3 => Self::Triangle,
            _ => Self::Sine,
        }
    }
}

/// One sample at `phase` cycles, stepping `increment` cycles a sample.
pub fn sample(waveform: Waveform, phase: f64, increment: f64) -> f32 {
    if !(0.0..0.5).contains(&increment) {
        return 0.0;
    }
    // Above a quarter of the rate no harmonic but the first is audible
    // without folding.
    if waveform == Waveform::Sine || increment >= 0.25 {
        return (phase * std::f64::consts::TAU).sin() as f32;
    }
    let half = (phase + 0.5).fract();
    let value = match waveform {
        Waveform::Saw => 2.0 * phase - 1.0 - blep(phase, increment),
        Waveform::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            naive + blep(phase, increment) - blep(half, increment)
        }
        Waveform::Triangle => {
            4.0 * (phase - 0.5).abs() - 1.0
                + 4.0 * increment * (blamp(half, increment) - blamp(phase, increment))
        }
        Waveform::Sine => unreachable!(),
    };
    value as f32
}

/// The step correction around an edge at phase 0.
fn blep(phase: f64, increment: f64) -> f64 {
    if phase < increment {
        let t = phase / increment - 1.0;
        -t * t
    } else if phase > 1.0 - increment {
        let t = (phase - 1.0) / increment + 1.0;
        t * t
    } else {
        0.0
    }
}

/// The corner correction around a slope change at phase 0.
fn blamp(phase: f64, increment: f64) -> f64 {
    if phase < increment {
        let t = phase / increment - 1.0;
        -t * t * t / 3.0
    } else if phase > 1.0 - increment {
        let t = (phase - 1.0) / increment + 1.0;
        t * t * t / 3.0
    } else {
        0.0
    }
}
