/// ADSR envelope stages.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum EnvelopeStage {
    Idle,
    Attack,
    Decay,
    Sustain,
    Release,
}

/// Per-voice ADSR envelope.
#[derive(Debug, Clone)]
pub(crate) struct Envelope {
    pub stage: EnvelopeStage,
    pub level: f32,
    pub attack: f32,
    pub decay: f32,
    pub sustain: f32,
    pub release: f32,
    pub sample_rate: f32,
}

impl Envelope {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            stage: EnvelopeStage::Idle,
            level: 0.0,
            attack: 0.01,
            decay: 0.1,
            sustain: 0.7,
            release: 0.3,
            sample_rate,
        }
    }

    pub fn trigger(&mut self) {
        self.stage = EnvelopeStage::Attack;
        // Don't reset level — allows retriggering without clicks
    }

    pub fn release(&mut self) {
        if self.stage != EnvelopeStage::Idle {
            self.stage = EnvelopeStage::Release;
        }
    }

    pub fn is_active(&self) -> bool {
        self.stage != EnvelopeStage::Idle
    }

    pub fn tick(&mut self) -> f32 {
        match self.stage {
            EnvelopeStage::Idle => 0.0,
            EnvelopeStage::Attack => {
                let rate = 1.0 / (self.attack * self.sample_rate).max(1.0);
                self.level += rate;
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = EnvelopeStage::Decay;
                }
                self.level
            }
            EnvelopeStage::Decay => {
                let rate = 1.0 / (self.decay * self.sample_rate).max(1.0);
                self.level -= rate * (1.0 - self.sustain);
                if self.level <= self.sustain {
                    self.level = self.sustain;
                    self.stage = EnvelopeStage::Sustain;
                }
                self.level
            }
            EnvelopeStage::Sustain => self.sustain,
            EnvelopeStage::Release => {
                let rate = 1.0 / (self.release * self.sample_rate).max(1.0);
                self.level -= rate * self.level.max(0.001);
                if self.level <= 0.001 {
                    self.level = 0.0;
                    self.stage = EnvelopeStage::Idle;
                }
                self.level
            }
        }
    }
}
