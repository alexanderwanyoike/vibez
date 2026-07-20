//! EngineTrack Note Repeat lifecycle and stopped-transport rendering.

use super::*;
use crate::note_repeat::{NoteRepeatClock, NoteRepeatStart};

impl EngineTrack {
    pub fn effective_swing(&self, project_swing: SwingAmount) -> SwingAmount {
        project_swing.effective(self.swing_offset)
    }

    pub(crate) fn start_note_repeat(&mut self, start: NoteRepeatStart, mut clock: NoteRepeatClock) {
        clock.swing = self.effective_swing(clock.swing);
        self.note_repeats.start(start, clock);
    }

    pub fn update_note_repeat_rate(
        &mut self,
        id: u8,
        rate: NoteRepeatRate,
        after_sample: u64,
        bpm: f64,
        sample_rate: u32,
        project_swing: SwingAmount,
    ) {
        let swing = self.effective_swing(project_swing);
        self.note_repeats
            .update_rate(id, rate, after_sample, bpm, sample_rate, swing);
    }

    pub fn stop_note_repeat(&mut self, id: u8) {
        self.note_repeats.stop(id);
    }

    pub fn reschedule_note_repeats(
        &mut self,
        after_sample: u64,
        bpm: f64,
        sample_rate: u32,
        project_swing: SwingAmount,
    ) {
        let swing = self.effective_swing(project_swing);
        self.note_repeats
            .reschedule(after_sample, bpm, sample_rate, swing);
    }

    pub fn render_instrument_idle(
        &mut self,
        repeat_pos: u64,
        frames: usize,
        channels: usize,
        tempo_map: &TempoMap,
        project_swing: SwingAmount,
        on_repeat: &mut dyn FnMut(NoteRepeatTrigger),
    ) -> bool {
        if self.instrument.is_none() {
            return false;
        }
        let buf_size = frames * channels;
        self.ensure_buffer(buf_size);
        self.mix_buffer[..buf_size].fill(0.0);
        let swing = self.effective_swing(project_swing);
        let bpm = tempo_map.bpm;
        let sample_rate = tempo_map.sample_rate;
        let batch = self.instrument.as_ref().unwrap().supports_batch_render();
        if batch {
            for frame in 0..frames {
                let (triggers, count) = self.note_repeats.triggers_at(
                    repeat_pos + frame as u64,
                    bpm,
                    sample_rate,
                    swing,
                );
                for trigger in triggers.into_iter().take(count).flatten() {
                    let instrument = self.instrument.as_mut().unwrap();
                    instrument.note_off_at(trigger.pitch, frame as u32);
                    instrument.note_on_at(trigger.pitch, trigger.velocity, frame as u32);
                    on_repeat(trigger);
                }
            }
            let instrument = self.instrument.as_mut().unwrap();
            instrument.render(&mut self.mix_buffer[..buf_size], channels);
        } else {
            for frame in 0..frames {
                let (triggers, count) = self.note_repeats.triggers_at(
                    repeat_pos + frame as u64,
                    bpm,
                    sample_rate,
                    swing,
                );
                let instrument = self.instrument.as_mut().unwrap();
                for trigger in triggers.into_iter().take(count).flatten() {
                    instrument.note_off(trigger.pitch);
                    instrument.note_on(trigger.pitch, trigger.velocity);
                    on_repeat(trigger);
                }
                let start = frame * channels;
                instrument.render(&mut self.mix_buffer[start..start + channels], channels);
            }
        }
        self.mix_buffer[..buf_size].iter().any(|&s| s != 0.0)
    }
}
