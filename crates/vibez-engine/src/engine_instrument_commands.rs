use super::*;

impl AudioEngine {
    pub(super) fn apply_instrument_command(&mut self, command: EngineCommand) {
        match command {
            // -- Instrument tracks --
            EngineCommand::AddInstrumentTrack(id, _name, kind) => {
                let mut track = EngineTrack::new(id);
                track.instrument = Some(create_instrument(kind, self.sample_rate as f32));
                self.tracks.push(track);
                self.recalculate_audio_length();
            }
            EngineCommand::AddMidiTrack(id, _name) => {
                self.tracks.push(EngineTrack::new(id));
                self.recalculate_audio_length();
            }
            EngineCommand::SetTrackInstrument(track_id, kind) => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    let old = track
                        .instrument
                        .replace(create_instrument(kind, self.sample_rate as f32));
                    if let Some(old) = old {
                        self.dispose_instrument(old);
                    }
                }
            }
            EngineCommand::RemoveTrackInstrument(track_id) => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(instrument) = track.instrument.take() {
                        self.dispose_instrument(instrument);
                    }
                }
            }
            EngineCommand::SetNoteClipDuration {
                track_id,
                clip_id,
                duration_beats,
            } => {
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
            EngineCommand::SetNoteClipGrooveGrid {
                track_id,
                clip_id,
                groove_grid,
            } => {
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
            EngineCommand::AddNoteClip {
                track_id,
                clip_id,
                position_beats,
                duration_beats,
                start_marker_beats,
                loop_enabled,
                loop_start_beats,
                loop_end_beats,
                groove_grid,
            } => {
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
            EngineCommand::RemoveNoteClip(track_id, clip_id) => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    track.playback_source.note_clips.retain(|c| c.id != clip_id);
                    // Sounding notes get their note-offs from the
                    // clip's schedule; without the clip they hang
                    // forever.
                    track.flush_notes();
                }
                self.recalculate_audio_length();
            }
            EngineCommand::MoveNoteClip {
                track_id,
                clip_id,
                new_position_beats,
            } => {
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
            EngineCommand::AddNote {
                track_id,
                clip_id,
                note,
            } => {
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
            EngineCommand::RemoveNote {
                track_id,
                clip_id,
                note_index,
            } => {
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
            EngineCommand::EditNote {
                track_id,
                clip_id,
                note_index,
                note,
            } => {
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
            EngineCommand::SetInstrumentParam {
                track_id,
                param_index,
                value,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut instrument) = track.instrument {
                        instrument.set_param(param_index, value);
                    }
                }
            }
            EngineCommand::LoadSamplerSample {
                track_id,
                sample,
                sample_name,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut instrument) = track.instrument {
                        instrument.load_sample(sample, sample_name);
                    }
                }
            }
            EngineCommand::LoadDrumRackPadSample {
                track_id,
                pad_index,
                sample,
                sample_name,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut instrument) = track.instrument {
                        instrument.load_drum_pad_sample(pad_index, sample, sample_name);
                    }
                }
            }
            EngineCommand::ClearDrumRackPad {
                track_id,
                pad_index,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut instrument) = track.instrument {
                        instrument.clear_drum_pad(pad_index);
                    }
                }
            }
            EngineCommand::SetDrumRackPadState {
                track_id,
                pad_index,
                state,
            } => {
                if let Some(track) = self.tracks.iter_mut().find(|t| t.id == track_id) {
                    if let Some(ref mut instrument) = track.instrument {
                        instrument.set_drum_pad_state(pad_index, state);
                    }
                }
            }

            _ => unreachable!("instrument command dispatch"),
        }
    }
}
