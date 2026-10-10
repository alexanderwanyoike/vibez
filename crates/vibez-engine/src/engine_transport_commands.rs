//! Typed audio-thread transport commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_stop(&mut self) {
        let was_clip_performance = self.clip_performance;
        self.stop_section_record();
        self.stop_heard_capture();
        self.clear_clip_performance();
        self.transport.stop();
        self.arrangement_recording = false;
        self.clock_domain = ClockDomain::Arrange;
        self.performance_position = self.transport.position();
        self.cancel_section_queue();
        self.cancel_queued_track_mutes();
        self.active_section = None;
        self.transport
            .set_audio_length(self.arrangement_audio_length);
        for track in &mut self.tracks {
            track.flush_notes();
        }
        self.stopped_note_repeat_anchor = self
            .has_active_note_repeats()
            .then_some(self.performance_position);
        if let Some(anchor) = self.stopped_note_repeat_anchor {
            self.reanchor_note_repeats(anchor, self.performance_position);
        }
        if was_clip_performance {
            self.clip_event(EngineEvent::PlaybackStopped);
        } else {
            let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
        }
    }

    pub(super) fn command_play(&mut self) {
        self.presentation_fault = false;
        if !self.transport.is_playing() {
            for track in self
                .tracks
                .iter_mut()
                .chain(self.buses.iter_mut())
                .chain(std::iter::once(&mut self.master))
            {
                for slot in &mut track.effects {
                    slot.effect.reset();
                }
            }
        }
        if let Some(routing) = self.routing.as_mut() {
            self.compensation_transition_frames = 0;
            routing.clear_history();
        }
        let was_clip_performance = self.clip_performance;
        self.clear_clip_performance();
        self.clock_domain = ClockDomain::Arrange;
        if !self.arrangement_recording {
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }
        self.transport.play();
        let audition_queued = self.audition.resync_on_transport_start(
            self.transport.position(),
            self.transport.bpm(),
            self.sample_rate,
            audition_fade_frames(self.sample_rate),
        );
        if let Some(queued) = audition_queued {
            let event = if queued {
                EngineEvent::AuditionQueued
            } else {
                EngineEvent::AuditionStarted
            };
            let _ = self.event_tx.push(event);
        }
        self.performance_position = self.transport.position();
        self.stopped_note_repeat_anchor = None;
        let anchor = self.playing_note_repeat_anchor();
        self.reanchor_note_repeats(anchor, self.performance_position);
        if was_clip_performance {
            self.clip_event(EngineEvent::PlaybackStarted);
        } else {
            let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
        }
    }

    pub(super) fn command_seek(&mut self, pos: u64) {
        if self.clock_domain == ClockDomain::Arrange {
            self.stop_heard_capture();
            if let Some(routing) = self.routing.as_mut() {
                self.compensation_transition_frames = 0;
                routing.clear_history();
            }
            for track in self
                .tracks
                .iter_mut()
                .chain(self.buses.iter_mut())
                .chain(std::iter::once(&mut self.master))
            {
                for slot in &mut track.effects {
                    slot.effect.reset();
                }
                if let Some(instrument) = track.instrument.as_mut() {
                    instrument.reset();
                }
            }
        }
        self.transport.seek(pos);
        for track in &mut self.tracks {
            track.flush_notes();
        }
        if self.clock_domain == ClockDomain::Arrange {
            self.performance_position = pos;
            self.reschedule_note_repeats();
        }
    }
}
