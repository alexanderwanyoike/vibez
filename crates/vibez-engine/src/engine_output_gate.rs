//! Output permission changes and plan publications preserve continuous output envelopes.

const TRANSITION_FRAMES: u32 = 64;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum OutputPermit {
    #[default]
    Audible,
    Muted,
}

#[derive(Default)]
pub(super) struct OutputGate {
    previous_permit: OutputPermit,
    last_frame: [f32; 2],
    // Recovery can arrive before loss finishes, and a plan morph can overlap
    // either envelope. Exclusive fade states would lose that pending work.
    loss_frames: u32,
    resume_frames: u32,
    morph_frames: u32,
}

impl OutputGate {
    pub(super) fn morph_after_plan_change(&mut self) {
        if self.previous_permit == OutputPermit::Audible {
            // Unchanged paths keep their own history. A short blend covers
            // newly prepared paths without gating the unrelated mix.
            self.morph_frames = TRANSITION_FRAMES;
        }
    }

    pub(super) fn clear_plan_morph(&mut self) {
        self.morph_frames = 0;
    }

    pub(super) fn apply(&mut self, output: &mut [f32], channels: usize, permit: OutputPermit) {
        if permit == OutputPermit::Muted && self.previous_permit == OutputPermit::Audible {
            self.loss_frames = TRANSITION_FRAMES;
            self.resume_frames = TRANSITION_FRAMES;
        }
        self.previous_permit = permit;
        if permit == OutputPermit::Muted {
            self.clear_plan_morph();
        }
        for frame in output.chunks_exact_mut(channels) {
            if self.loss_frames > 0 {
                let gain = self.loss_frames as f32 / TRANSITION_FRAMES as f32;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.last_frame[channel.min(1)] * gain;
                }
                self.loss_frames -= 1;
            } else if permit == OutputPermit::Audible && self.resume_frames > 0 {
                let gain = 1.0 - self.resume_frames as f32 / TRANSITION_FRAMES as f32;
                frame.iter_mut().for_each(|sample| *sample *= gain);
                self.resume_frames -= 1;
            }
            if permit == OutputPermit::Audible && self.morph_frames > 0 {
                let blend = 1.0 - self.morph_frames as f32 / TRANSITION_FRAMES as f32;
                for (channel, sample) in frame.iter_mut().enumerate() {
                    *sample = self.last_frame[channel.min(1)] * (1.0 - blend) + *sample * blend;
                }
                self.morph_frames -= 1;
            }
            if self.loss_frames == 0 && self.morph_frames == 0 {
                self.last_frame = [frame[0], frame[channels.min(2) - 1]];
            }
        }
    }

    #[cfg(test)]
    pub(super) fn last_frame(&self) -> [f32; 2] {
        self.last_frame
    }

    #[cfg(test)]
    pub(super) fn remaining_loss_frames(&self) -> u32 {
        self.loss_frames
    }

    #[cfg(test)]
    pub(super) fn remaining_resume_frames(&self) -> u32 {
        self.resume_frames
    }

    #[cfg(test)]
    pub(super) fn remaining_morph_frames(&self) -> u32 {
        self.morph_frames
    }
}
