//! Deterministic timing probes shared by engine and downstream acceptance tests.

use std::sync::{
    atomic::{AtomicU32, Ordering},
    Arc,
};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    midi::InstrumentKind,
};
use vibez_dsp::{compensation_delay::CompensationDelay, effect::AudioEffect};
use vibez_instruments::Instrument;

pub struct DelayProbe {
    line: CompensationDelay,
    report: Arc<AtomicU32>,
}

impl DelayProbe {
    pub fn new(frames: u32) -> Self {
        Self::with_report(frames, frames)
    }

    pub fn with_report(actual: u32, reported: u32) -> Self {
        Self::with_shared_report(actual, Arc::new(AtomicU32::new(reported)))
    }

    pub fn with_shared_report(actual: u32, report: Arc<AtomicU32>) -> Self {
        Self {
            line: Self::line(actual),
            report,
        }
    }

    pub fn set_report(&self, frames: u32) {
        self.report.store(frames, Ordering::Release);
    }

    pub fn set_actual_delay(&mut self, frames: u32) {
        self.line = Self::line(frames);
    }

    fn line(frames: u32) -> CompensationDelay {
        CompensationDelay::prepare(frames, 2, frames as usize * 2).unwrap()
    }
}

impl AudioEffect for DelayProbe {
    fn latency_samples(&self) -> u32 {
        self.report.load(Ordering::Acquire)
    }
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn process(&mut self, samples: &mut [f32], channels: usize) {
        self.line.process_layout(samples, channels);
    }
    fn reset(&mut self) {
        self.line.clear();
    }
}

pub struct DelayProbeInstrument {
    delay: DelayProbe,
    amplitude: f32,
    gate: bool,
}

impl DelayProbeInstrument {
    pub fn new(frames: u32, amplitude: f32) -> Self {
        Self {
            delay: DelayProbe::new(frames),
            amplitude,
            gate: false,
        }
    }
}

impl Instrument for DelayProbeInstrument {
    fn latency_samples(&self) -> u32 {
        self.delay.latency_samples()
    }
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn note_on(&mut self, _: u8, _: u8) {
        self.gate = true;
    }
    fn note_off(&mut self, _: u8) {
        self.gate = false;
    }
    fn render(&mut self, buffer: &mut [f32], channels: usize) {
        buffer.fill(if self.gate { self.amplitude } else { 0.0 });
        self.delay.process(buffer, channels);
    }
    fn reset(&mut self) {
        self.gate = false;
        self.delay.reset();
    }
}
