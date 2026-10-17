//! Typed audio-thread transport commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_stop(&mut self, allowance: &mut RetirementAllowance) {
        let capture_needs_closure = self.capture_needs_closure();
        self.cancel_presentation();
        self.stop_section_record();
        if capture_needs_closure {
            self.stop_heard_capture(allowance);
        }
        self.clear_clip_performance(allowance);
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
        self.present_event(EngineEvent::PlaybackStopped, allowance);
    }

    pub(super) fn command_play(&mut self, allowance: &mut RetirementAllowance) {
        let capture_needs_closure = self.capture_needs_closure();
        self.cancel_presentation();
        if capture_needs_closure {
            self.stop_heard_capture(allowance);
        }
        self.presentation_state.terminals.cancel_playback_stop();
        self.output_gate.clear_plan_morph();
        self.presentation_state.resume();
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
        self.routing.clear_history();
        let was_clip_performance = self.clip_performance;
        self.clear_clip_performance(allowance);
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
            self.present_event(event, allowance);
        }
        self.performance_position = self.transport.position();
        self.stopped_note_repeat_anchor = None;
        let anchor = self.playing_note_repeat_anchor();
        self.reanchor_note_repeats(anchor, self.performance_position);
        if was_clip_performance {
            self.clip_event(EngineEvent::PlaybackStarted, allowance);
        } else {
            self.present_event(EngineEvent::PlaybackStarted, allowance);
        }
    }

    pub(super) fn command_seek(&mut self, pos: u64, allowance: &mut RetirementAllowance) {
        if self.clock_domain == ClockDomain::Arrange && self.capture_needs_closure() {
            self.stop_heard_capture(allowance);
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
