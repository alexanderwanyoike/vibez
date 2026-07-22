//! Allocation-free Note Repeat scheduling on the engine clock.

use vibez_core::perform::{GrooveProfile, NoteRepeatRate, SwingAmount};

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
    pub anchor_sample: u64,
    pub include_after_sample: bool,
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
    anchor_sample: u64,
    next_sample: u64,
    next_step: u64,
}

#[derive(Debug, Default)]
pub struct TrackNoteRepeats {
    voices: [Option<NoteRepeatVoice>; MAX_NOTE_REPEATS],
}

impl TrackNoteRepeats {
    pub(crate) fn start(&mut self, start: NoteRepeatStart, clock: NoteRepeatClock) {
        let (next_sample, next_step) = next_repeat_from_anchor(
            clock.after_sample,
            clock.anchor_sample,
            start.rate,
            clock.bpm,
            clock.sample_rate,
            clock.swing,
            clock.include_after_sample,
        );
        let voice = NoteRepeatVoice {
            id: start.id,
            pitch: start.pitch,
            velocity: start.velocity,
            rate: start.rate,
            anchor_sample: clock.anchor_sample,
            next_sample,
            next_step,
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

    pub const fn is_active(&self) -> bool {
        let mut index = 0;
        while index < self.voices.len() {
            if self.voices[index].is_some() {
                return true;
            }
            index += 1;
        }
        false
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
            (voice.next_sample, voice.next_step) = next_repeat_from_anchor(
                after_sample,
                voice.anchor_sample,
                rate,
                bpm,
                sample_rate,
                swing,
                false,
            );
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
            (voice.next_sample, voice.next_step) = next_repeat_from_anchor(
                after_sample,
                voice.anchor_sample,
                voice.rate,
                bpm,
                sample_rate,
                swing,
                false,
            );
        }
    }

    pub fn reanchor(
        &mut self,
        anchor_sample: u64,
        after_sample: u64,
        bpm: f64,
        sample_rate: u32,
        swing: SwingAmount,
    ) {
        for voice in self.voices.iter_mut().flatten() {
            voice.anchor_sample = anchor_sample;
            (voice.next_sample, voice.next_step) = next_repeat_from_anchor(
                after_sample,
                anchor_sample,
                voice.rate,
                bpm,
                sample_rate,
                swing,
                true,
            );
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
            let previous_sample = voice.next_sample;
            voice.next_step = voice.next_step.saturating_add(1);
            voice.next_sample = repeat_sample_for_step(
                voice.anchor_sample,
                voice.rate,
                bpm,
                sample_rate,
                swing,
                voice.next_step,
            )
            .unwrap_or_else(|| previous_sample.saturating_add(1))
            .max(previous_sample.saturating_add(1));
        }
        (triggers, count)
    }
}

/// First MPC2000XL subdivision strictly after `after_sample`. The profile uses
/// a 96 PPQN clock and interprets Swing as the long side of a two-step ratio.
pub fn next_repeat_sample(
    after_sample: u64,
    rate: NoteRepeatRate,
    bpm: f64,
    sample_rate: u32,
    swing: SwingAmount,
) -> u64 {
    next_repeat_from_anchor(after_sample, 0, rate, bpm, sample_rate, swing, false).0
}

fn next_repeat_from_anchor(
    after_sample: u64,
    anchor_sample: u64,
    rate: NoteRepeatRate,
    bpm: f64,
    sample_rate: u32,
    swing: SwingAmount,
    include_after_sample: bool,
) -> (u64, u64) {
    if !bpm.is_finite() || bpm <= 0.0 || sample_rate == 0 {
        return (after_sample.saturating_add(1), 0);
    }
    let samples_per_tick =
        f64::from(sample_rate) * 60.0 / (bpm * f64::from(GrooveProfile::MPC2000XL_PPQN));
    let ticks_per_step = rate.interval_beats() * f64::from(GrooveProfile::MPC2000XL_PPQN);
    if !samples_per_tick.is_finite()
        || samples_per_tick <= 0.0
        || !ticks_per_step.is_finite()
        || ticks_per_step <= 0.0
    {
        return (after_sample.saturating_add(1), 0);
    }
    let relative_after = after_sample.saturating_sub(anchor_sample);
    let approximate_step = (relative_after as f64 / samples_per_tick / ticks_per_step)
        .floor()
        .max(0.0) as u64;
    for step in approximate_step..approximate_step.saturating_add(4) {
        let sample = repeat_sample_for_step(anchor_sample, rate, bpm, sample_rate, swing, step)
            .expect("validated repeat clock");
        if sample > after_sample || (include_after_sample && sample == after_sample) {
            return (sample, step);
        }
    }
    (
        after_sample.saturating_add((ticks_per_step * samples_per_tick).round().max(1.0) as u64),
        approximate_step.saturating_add(4),
    )
}

fn repeat_sample_for_step(
    anchor_sample: u64,
    rate: NoteRepeatRate,
    bpm: f64,
    sample_rate: u32,
    swing: SwingAmount,
    step: u64,
) -> Option<u64> {
    if !bpm.is_finite() || bpm <= 0.0 || sample_rate == 0 {
        return None;
    }
    let samples_per_tick =
        f64::from(sample_rate) * 60.0 / (bpm * f64::from(GrooveProfile::MPC2000XL_PPQN));
    let ticks_per_step = rate.interval_beats() * f64::from(GrooveProfile::MPC2000XL_PPQN);
    if !samples_per_tick.is_finite()
        || samples_per_tick <= 0.0
        || !ticks_per_step.is_finite()
        || ticks_per_step <= 0.0
    {
        return None;
    }
    let position_ticks = if GrooveProfile::Mpc2000XlV1.swings(rate) && step % 2 == 1 {
        let pair_start = step.saturating_sub(1) as f64 * ticks_per_step;
        pair_start + (2.0 * ticks_per_step * f64::from(swing.get())).round()
    } else {
        step as f64 * ticks_per_step
    };
    Some(anchor_sample.saturating_add((position_ticks * samples_per_tick).round().max(0.0) as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mpc2000xl_swing_uses_native_ratios_on_a_96_ppqn_clock() {
        for (rate, straight, near_triplet, three_to_one, pair_end) in [
            (NoteRepeatRate::Eighth, 48, 63, 72, 96),
            (NoteRepeatRate::Sixteenth, 24, 32, 36, 48),
        ] {
            assert_eq!(
                next_repeat_sample(0, rate, 60.0, 96, SwingAmount::new(0.50)),
                straight
            );
            assert_eq!(
                next_repeat_sample(0, rate, 60.0, 96, SwingAmount::new(0.66)),
                near_triplet
            );
            assert_eq!(
                next_repeat_sample(0, rate, 60.0, 96, SwingAmount::new(0.75)),
                three_to_one
            );
            assert_eq!(
                next_repeat_sample(three_to_one, rate, 60.0, 96, SwingAmount::new(0.75)),
                pair_end
            );
        }
    }

    #[test]
    fn mpc2000xl_swing_only_applies_to_eighth_and_sixteenth_grids() {
        for rate in [NoteRepeatRate::Quarter, NoteRepeatRate::ThirtySecond] {
            let straight = next_repeat_sample(0, rate, 60.0, 96, SwingAmount::STRAIGHT);
            let swung = next_repeat_sample(0, rate, 60.0, 96, SwingAmount::new(0.75));
            assert_eq!(swung, straight, "{rate} must remain unswung");
        }
    }

    #[test]
    fn triplets_are_exact_at_every_swing_amount() {
        for rate in [
            NoteRepeatRate::QuarterTriplet,
            NoteRepeatRate::EighthTriplet,
            NoteRepeatRate::SixteenthTriplet,
            NoteRepeatRate::ThirtySecondTriplet,
        ] {
            let straight = next_repeat_sample(0, rate, 60.0, 300, SwingAmount::STRAIGHT);
            let swung = next_repeat_sample(0, rate, 60.0, 300, SwingAmount::new(0.75));
            assert_eq!(swung, straight, "{rate} must remain unswung");
        }
    }
}
