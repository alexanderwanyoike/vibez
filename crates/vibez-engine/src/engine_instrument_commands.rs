//! Typed audio-thread instrument commands handlers.

use super::*;

impl AudioEngine {
    pub(super) fn command_add_instrument_track(
        &mut self,
        id: vibez_core::id::TrackId,
        _name: String,
        kind: vibez_core::midi::InstrumentKind,
    ) {
        let mut track = EngineTrack::new(id);
        track.instrument = Some(create_instrument(kind, self.sample_rate as f32));
        self.tracks.push(track);
        self.recalculate_audio_length();
    }

    pub(super) fn command_add_midi_track(&mut self, id: vibez_core::id::TrackId, _name: String) {
        self.tracks.push(EngineTrack::new(id));
        self.recalculate_audio_length();
    }

    pub(super) fn command_set_track_instrument(
        &mut self,
        track_id: vibez_core::id::TrackId,
        kind: vibez_core::midi::InstrumentKind,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            let old = track
                .instrument
                .replace(create_instrument(kind, self.sample_rate as f32));
            if let Some(old) = old {
                self.dispose_instrument(old);
            }
        }
    }

    pub(super) fn command_remove_track_instrument(&mut self, track_id: vibez_core::id::TrackId) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(instrument) = track.instrument.take() {
                self.dispose_instrument(instrument);
            }
        }
    }

    pub(super) fn command_set_note_clip_duration(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        duration_beats: f64,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|c| c.id == clip_id)
            {
                clip.duration_beats = duration_beats;
            }
            track.flush_notes();
        }
        self.recalculate_audio_length();
    }

    pub(super) fn command_set_note_clip_groove_grid(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        groove_grid: vibez_core::perform::GrooveGrid,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|clip| clip.id == clip_id)
            {
                clip.groove_grid = groove_grid;
            }
            track.flush_notes();
        }
    }

    // Explicit fields keep the public command dispatch compiler checked.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn command_add_note_clip(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        position_beats: f64,
        duration_beats: f64,
        start_marker_beats: f64,
        loop_enabled: bool,
        loop_start_beats: f64,
        loop_end_beats: f64,
        groove_grid: vibez_core::perform::GrooveGrid,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.playback_source.note_clips.push(EngineNoteClip::new(
                clip_id,
                position_beats,
                duration_beats,
                Vec::new(),
                start_marker_beats,
                loop_enabled,
                loop_start_beats,
                loop_end_beats,
                groove_grid,
            ));
        }
        self.recalculate_audio_length();
    }

    pub(super) fn command_remove_note_clip(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            track.playback_source.note_clips.retain(|c| c.id != clip_id);
            // Sounding notes get their note-offs from the
            // clip's schedule; without the clip they hang
            // forever.
            track.flush_notes();
        }
        self.recalculate_audio_length();
    }

    pub(super) fn command_move_note_clip(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        new_position_beats: f64,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|c| c.id == clip_id)
            {
                clip.position_beats = new_position_beats;
            }
        }
        self.recalculate_audio_length();
    }

    pub(super) fn command_add_note(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        note: vibez_core::midi::MidiNote,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|c| c.id == clip_id)
            {
                clip.push_note(note);
            }
        }
    }

    pub(super) fn command_remove_note(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        note_index: usize,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|c| c.id == clip_id)
            {
                clip.remove_note(note_index);
            }
            track.flush_notes();
        }
    }

    pub(super) fn command_edit_note(
        &mut self,
        track_id: vibez_core::id::TrackId,
        clip_id: vibez_core::id::ClipId,
        note_index: usize,
        note: vibez_core::midi::MidiNote,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(clip) = track
                .playback_source
                .note_clips
                .iter_mut()
                .find(|c| c.id == clip_id)
            {
                clip.edit_note(note_index, note);
            }
            track.flush_notes();
        }
    }

    pub(super) fn command_set_instrument_param(
        &mut self,
        track_id: vibez_core::id::TrackId,
        param_index: usize,
        value: f32,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(ref mut instrument) = track.instrument {
                instrument.set_param(param_index, value);
            }
        }
    }

    pub(super) fn command_load_sampler_sample(
        &mut self,
        track_id: vibez_core::id::TrackId,
        sample: std::sync::Arc<vibez_core::audio_buffer::DecodedAudio>,
        sample_name: String,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(ref mut instrument) = track.instrument {
                instrument.load_sample(sample, sample_name);
            }
        }
    }

    pub(super) fn command_load_drum_rack_pad_sample(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pad_index: usize,
        sample: std::sync::Arc<vibez_core::audio_buffer::DecodedAudio>,
        sample_name: String,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(ref mut instrument) = track.instrument {
                instrument.load_drum_pad_sample(pad_index, sample, sample_name);
            }
        }
    }

    pub(super) fn command_clear_drum_rack_pad(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pad_index: usize,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(ref mut instrument) = track.instrument {
                instrument.clear_drum_pad(pad_index);
            }
        }
    }

    pub(super) fn command_set_drum_rack_pad_state(
        &mut self,
        track_id: vibez_core::id::TrackId,
        pad_index: usize,
        state: vibez_core::track::DrumPadState,
    ) {
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
            if let Some(ref mut instrument) = track.instrument {
                instrument.set_drum_pad_state(pad_index, state);
            }
        }
    }
}
