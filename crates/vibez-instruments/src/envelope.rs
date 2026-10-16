//! The instruments' ADSR: a straight attack, then decay and release that fall
//! exponentially and reach -60 dB of their distance in exactly their time,
//! the way analogue envelopes and most soft synths read their knobs.

/// -60 dB, where a decay or release counts as done.
const DONE: f32 = 0.001;
/// Below this a released voice is silent and stops.
const SILENT: f32 = 0.0001;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Stage {
    #[default]
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Each stage's per-sample step, worked out once when a time changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Times {
    attack_step: f32,
    decay_falloff: f32,
    sustain: f32,
    release_falloff: f32,
}

impl Times {
    pub fn new(attack: f32, decay: f32, sustain: f32, release: f32, sample_rate: f32) -> Self {
        Self {
            attack_step: 1.0 / (attack * sample_rate).max(1.0),
            decay_falloff: falloff(decay, sample_rate),
            sustain: sustain.clamp(0.0, 1.0),
            release_falloff: falloff(release, sample_rate),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Envelope {
    stage: Stage,
    level: f32,
}

/// The per-sample multiplier that brings a distance down to -60 dB in
/// `seconds`.
fn falloff(seconds: f32, sample_rate: f32) -> f32 {
    DONE.powf(1.0 / (seconds * sample_rate).max(1.0))
}

impl Envelope {
    /// Starts again from wherever the level is, so a retrigger never clicks.
    pub fn trigger(&mut self) {
        self.stage = Stage::Attack;
    }

    pub fn release(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    pub fn released(&self) -> bool {
        matches!(self.stage, Stage::Release | Stage::Idle)
    }

    pub fn tick(&mut self, times: &Times) -> f32 {
        match self.stage {
            Stage::Idle => return 0.0,
            Stage::Attack => {
                self.level += times.attack_step;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                let sustain = times.sustain;
                self.level = sustain + (self.level - sustain) * times.decay_falloff;
                if (self.level - sustain).abs() <= DONE * (1.0 - sustain).max(DONE) {
                    self.level = sustain;
                    self.stage = Stage::Sustain;
                }
            }
            Stage::Sustain => self.level = times.sustain,
            Stage::Release => {
                self.level *= times.release_falloff;
                if self.level < SILENT {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}
