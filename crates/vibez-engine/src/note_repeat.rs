//! Allocation-free Note Repeat scheduling on the engine clock.

use vibez_core::perform::{NoteRepeatRate, SwingAmount};

pub const MAX_NOTE_REPEATS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoteRepeatTrigger {
    pub pitch: u8,
    pub velocity: u8,
    pub effective_at_samples: u64,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteRepeatStart {
    pub id: u8,
    pub pitch: u8,
    pub velocity: u8,
    pub rate: NoteRepeatRate,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct NoteRepeatClock {
    pub after_sample: u64,
    pub bpm: f64,
    pub sample_rate: u32,
    pub swing: SwingAmount,
}

#[derive(Debug, Clone, Copy)]
struct NoteRepeatVoice {
    id: u8,
    pitch: u8,
    velocity: u8,
    rate: NoteRepeatRate,
    next_sample: u64,
}

#[derive(Debug, Default)]
pub struct TrackNoteRepeats {
    voices: [Option<NoteRepeatVoice>; MAX_NOTE_REPEATS],
}

impl TrackNoteRepeats {
    pub(crate) fn start(&mut self, start: NoteRepeatStart, clock: NoteRepeatClock) {
        let voice = NoteRepeatVoice {
            id: start.id,
            pitch: start.pitch,
            velocity: start.velocity,
            rate: start.rate,
            next_sample: next_repeat_sample(
                clock.after_sample,
                start.rate,
                clock.bpm,
                clock.sample_rate,
                clock.swing,
            ),
        };
        if let Some(existing) = self
            .voices
            .iter_mut()
            .find(|candidate| candidate.is_some_and(|candidate| candidate.id == start.id))
        {
            *existing = Some(voice);
        } else if let Some(vacant) = self.voices.iter_mut().find(|candidate| candidate.is_none()) {
            *vacant = Some(voice);
        }
    }

    pub fn stop(&mut self, id: u8) {
        if let Some(voice) = self
            .voices
            .iter_mut()
            .find(|candidate| candidate.is_some_and(|candidate| candidate.id == id))
        {
            *voice = None;
        }
    }

    pub fn update_rate(
        &mut self,
        id: u8,
        rate: NoteRepeatRate,
        after_sample: u64,
        bpm: f64,
        sample_rate: u32,
        swing: SwingAmount,
    ) {
        if let Some(voice) = self
            .voices
            .iter_mut()
            .flatten()
            .find(|voice| voice.id == id)
        {
            voice.rate = rate;
            voice.next_sample = next_repeat_sample(after_sample, rate, bpm, sample_rate, swing);
        }
    }

    pub fn reschedule(
        &mut self,
        after_sample: u64,
        bpm: f64,
        sample_rate: u32,
        swing: SwingAmount,
    ) {
        for voice in self.voices.iter_mut().flatten() {
            voice.next_sample =
                next_repeat_sample(after_sample, voice.rate, bpm, sample_rate, swing);
        }
    }

    pub fn triggers_at(
        &mut self,
        sample: u64,
        bpm: f64,
        sample_rate: u32,
        swing: SwingAmount,
    ) -> ([Option<NoteRepeatTrigger>; MAX_NOTE_REPEATS], usize) {
        let mut triggers = [None; MAX_NOTE_REPEATS];
        let mut count = 0;
        for voice in self.voices.iter_mut().flatten() {
            if voice.next_sample > sample {
                continue;
            }
            triggers[count] = Some(NoteRepeatTrigger {
                pitch: voice.pitch,
                velocity: voice.velocity,
                effective_at_samples: voice.next_sample,
            });
            count += 1;
            voice.next_sample =
                next_repeat_sample(voice.next_sample, voice.rate, bpm, sample_rate, swing);
        }
        (triggers, count)
    }
}

/// First subdivision strictly after `after_sample`. Straight-grid odd steps
/// are delayed toward the triplet position; triplet grids ignore Swing.
pub fn next_repeat_sample(
    after_sample: u64,
    rate: NoteRepeatRate,
    bpm: f64,
    sample_rate: u32,
    swing: SwingAmount,
) -> u64 {
    if !bpm.is_finite() || bpm <= 0.0 || sample_rate == 0 {
        return after_sample.saturating_add(1);
    }
    let interval = rate.interval_beats() * f64::from(sample_rate) * 60.0 / bpm;
    if !interval.is_finite() || interval <= 0.0 {
        return after_sample.saturating_add(1);
    }
    let approximate_step = (after_sample as f64 / interval).floor().max(0.0) as u64;
    for step in approximate_step..approximate_step.saturating_add(4) {
        let mut position = step as f64 * interval;
        if !rate.is_triplet() && step % 2 == 1 {
            position += f64::from(swing.get()) * interval / 3.0;
        }
        let sample = position.round().max(0.0) as u64;
        if sample > after_sample {
            return sample;
        }
    }
    after_sample.saturating_add(interval.round().max(1.0) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straight_swing_delays_only_offbeats() {
        let swing = SwingAmount::new(1.0);
        assert_eq!(
            next_repeat_sample(0, NoteRepeatRate::Eighth, 60.0, 100, swing),
            67
        );
        assert_eq!(
            next_repeat_sample(67, NoteRepeatRate::Eighth, 60.0, 100, swing),
            100
        );
        assert_eq!(
            next_repeat_sample(100, NoteRepeatRate::Eighth, 60.0, 100, swing),
            167
        );
    }

    #[test]
    fn triplets_are_exact_at_every_swing_amount() {
        let straight = next_repeat_sample(
            0,
            NoteRepeatRate::EighthTriplet,
            60.0,
            300,
            SwingAmount::STRAIGHT,
        );
        let swung = next_repeat_sample(
            0,
            NoteRepeatRate::EighthTriplet,
            60.0,
            300,
            SwingAmount::new(1.0),
        );
        assert_eq!(straight, 100);
        assert_eq!(swung, straight);
    }
}
