//! Two inline histories bound manual transitions while automation supersedes each field.

use super::{clock::Clock, param, random::Random};

const TRANSITION_FRAMES: u16 = 512;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Configuration {
    bits: u32,
    divider: u32,
    rounding: u32,
    dither: bool,
}

impl Configuration {
    pub(super) fn from_params(params: &[f32; param::COUNT]) -> Self {
        Self {
            bits: params[param::BITS] as u32,
            divider: params[param::SR_REDUCE].round() as u32,
            rounding: params[param::ROUNDING] as u32,
            dither: params[param::DITHER] != 0.0,
        }
    }

    fn edit(&mut self, index: usize, value: f32) {
        match index {
            param::BITS => self.bits = value as u32,
            param::SR_REDUCE => self.divider = value.round() as u32,
            param::ROUNDING => self.rounding = value as u32,
            param::DITHER => self.dither = value != 0.0,
            _ => unreachable!(),
        }
    }
}

pub(super) struct Source<'a> {
    pub dry: [f32; 2],
    pub channels: usize,
    pub drive_db: f32,
    pub drive_gain: f64,
    pub jitter: f32,
    pub frame: u64,
    pub random: &'a Random,
}

pub(super) struct Frame {
    pub wet: [f32; 2],
    pub primary_acquired: bool,
    pub secondary_acquired: bool,
    pub transition: bool,
    pub primary_interval: u32,
    pub secondary_interval: u32,
    pub transition_active: bool,
}

#[derive(Clone, Copy)]
struct Lane {
    configuration: Configuration,
    hold: [f32; 2],
    clock: Clock,
}

impl Lane {
    fn new(configuration: Configuration) -> Self {
        Self { configuration, hold: [0.0; 2], clock: Clock::default() }
    }

    fn process(&mut self, source: &Source<'_>) -> bool {
        let acquired = self.clock.advance(
            self.configuration.divider, source.jitter, source.frame, source.random,
        );
        if acquired {
            let step = 2.0 / (1_u32 << self.configuration.bits) as f32;
            for channel in 0..source.channels {
                // Saved projects retain their original f32 division at neutral drive/dither.
                let scaled = if source.drive_db == 0.0 && !self.configuration.dither {
                    f64::from(source.dry[channel] / step)
                } else {
                    f64::from(source.dry[channel]) * source.drive_gain / f64::from(step)
                };
                let scaled = if self.configuration.dither {
                    scaled + source.random.tpdf(source.frame, channel)
                } else {
                    scaled
                };
                let code = match self.configuration.rounding {
                    1 => scaled.floor(),
                    2 => scaled.trunc(),
                    _ => scaled.round(),
                };
                self.hold[channel] = (code * f64::from(step)).clamp(-1.0, 1.0) as f32;
            }
        }
        acquired
    }
}

pub(super) struct Lanes {
    lanes: [Lane; 2],
    current: usize,
    phase: u16,
    pending: Option<Configuration>,
}

impl Lanes {
    pub(super) fn new(configuration: Configuration) -> Self {
        Self { lanes: [Lane::new(configuration); 2], current: 0, phase: 0,
            pending: None }
    }

    pub(super) fn edit(&mut self, index: usize, value: f32, immediate: bool) {
        if !matches!(index, param::BITS | param::SR_REDUCE | param::ROUNDING | param::DITHER) {
            return;
        }
        if immediate {
            for lane in &mut self.lanes {
                lane.configuration.edit(index, value);
            }
            if let Some(pending) = &mut self.pending {
                pending.edit(index, value);
            }
        }
        let destination = if self.phase == 0 { self.current } else { 1 - self.current };
        let mut target = self.pending.unwrap_or(self.lanes[destination].configuration);
        target.edit(index, value);
        self.pending = (target != self.lanes[destination].configuration).then_some(target);
    }

    pub(super) fn process(&mut self, source: &Source<'_>) -> Frame {
        if self.phase == 0 {
            if let Some(configuration) = self.pending.take() {
                let incoming = 1 - self.current;
                self.lanes[incoming] = self.lanes[self.current];
                self.lanes[incoming].configuration = configuration;
                self.phase = 1;
            }
        }
        let primary_acquired = self.lanes[self.current].process(source);
        let mut wet = self.lanes[self.current].hold;
        let transition = self.phase != 0;
        let mut secondary_acquired = false;
        if transition {
            let incoming = 1 - self.current;
            secondary_acquired = self.lanes[incoming].process(source);
            let amount = f32::from(self.phase) / f32::from(TRANSITION_FRAMES);
            for (channel, sample) in wet.iter_mut().enumerate().take(source.channels) {
                *sample = *sample * (1.0 - amount) + self.lanes[incoming].hold[channel] * amount;
            }
            if self.phase == TRANSITION_FRAMES {
                self.current = incoming;
                self.phase = 0;
            } else {
                self.phase += 1;
            }
        }
        Frame { wet, primary_acquired, secondary_acquired, transition,
            primary_interval: self.lanes[self.current].clock.interval(),
            secondary_interval: if self.phase == 0 { 0 } else { self.lanes[1 - self.current].clock.interval() },
            transition_active: self.phase != 0 }
    }
}
