//! Convert a marked Audio Clip into a native Drum Rack and reconstruction MIDI Clip.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::perform::GrooveGrid;
use vibez_core::track::{drum_rack_pad_pitch, MediaSourceRef, DRUM_RACK_PAD_COUNT};

use crate::state::{
    ArrangementSelection, ProjectTracksState, TimelineEditorState, UiDrumPad, UiNoteClip,
};

use super::slicing::{marker_cut_positions, slice_boundaries};
use super::{ArrangementAction, ArrangementCtx, AudioSliceMarkers};

const SLICE_VELOCITY: u8 = 127;

pub(super) struct DrumRackSliceMaterial {
    pub markers: AudioSliceMarkers,
    pub source: MediaSourceRef,
    pub audio: Arc<DecodedAudio>,
}

impl TimelineEditorState {
    pub(super) fn slice_audio_clip_to_drum_rack(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        source_track_id: TrackId,
        source_clip_id: ClipId,
        material: DrumRackSliceMaterial,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let DrumRackSliceMaterial {
            markers,
            source,
            audio,
        } = material;
        let Some(original) = self
            .find_content(source_track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == source_clip_id))
            .cloned()
        else {
            return ArrangementAction::default();
        };
        if usize::try_from(original.duration) != Ok(audio.num_frames())
            || ctx.samples_per_beat <= 0.0
        {
            return failure("Slice to Drum Rack needs available audio and a valid tempo");
        }

        let cuts = marker_cut_positions(&original, markers);
        if cuts.is_empty() {
            return failure(&format!(
                "Add at least one interior {} first",
                marker_name(markers)
            ));
        }
        let boundaries = slice_boundaries(&original, cuts);
        let total_regions = boundaries.len() - 1;
        if total_regions > DRUM_RACK_PAD_COUNT {
            return failure(&format!(
                "{total_regions} slices exceed the {DRUM_RACK_PAD_COUNT}-pad Drum Rack; reduce the markers first"
            ));
        }
        let source_frames = audio.num_frames() as f32;
        let mut pads = Vec::with_capacity(boundaries.len() - 1);
        let mut notes = Vec::with_capacity(boundaries.len() - 1);
        for (index, boundary) in boundaries.windows(2).enumerate() {
            let local_start = boundary[0];
            let duration = boundary[1] - boundary[0];
            let source_start = local_start;
            let source_end = local_start.saturating_add(duration);
            if source_end <= source_start {
                return failure("A marker produced an empty Drum Rack slice");
            }
            pads.push(UiDrumPad {
                name: Some(format!("{} Slice {}", original.name, index + 1)),
                source: Some(source.clone()),
                audio: Some(Arc::clone(&audio)),
                gain: original.gain_db.linear(),
                pan: 0.0,
                start: source_start as f32 / source_frames,
                end: source_end as f32 / source_frames,
                coarse_tune: 0,
                fine_tune: 0.0,
                one_shot: true,
                choke_group: None,
            });
            notes.push(MidiNote {
                pitch: drum_rack_pad_pitch(index).expect("slice count is capped to the rack"),
                velocity: SLICE_VELOCITY,
                start_beat: local_start as f64 / ctx.samples_per_beat,
                duration_beats: duration as f64 / ctx.samples_per_beat,
            });
        }

        let mut no_engine = crate::domains::DiscardingEngine;
        let track_id = project_tracks.add_numbered_track("Slices", TrackKind::Midi, &mut no_engine);
        let track = project_tracks
            .find_mut(track_id)
            .expect("new Project Track");
        let track_name = track.name.clone();
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::DrumRack);
        for (slot, pad) in track.drum_rack_pads.iter_mut().zip(pads) {
            *slot = pad;
        }

        let note_clip_id = ClipId::new();
        let duration_beats = original.duration as f64 / ctx.samples_per_beat;
        let note_clip = UiNoteClip {
            id: note_clip_id,
            name: format!("{} Slices", original.name),
            position_beats: original.position as f64 / ctx.samples_per_beat,
            duration_beats,
            notes,
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: duration_beats,
            groove_grid: GrooveGrid::Off,
        };

        Arc::make_mut(&mut self.timeline)
            .ensure(track_id)
            .note_clips
            .push(note_clip);
        self.selected_track = Some(track_id);
        self.selected_clips.clear();
        self.selected_clips.insert(ArrangementSelection::NoteClip {
            track_id,
            clip_id: note_clip_id,
        });
        self.selected_note_clip = Some((track_id, note_clip_id));

        ArrangementAction {
            replay_project_track: Some(track_id),
            status: Some(format!(
                "Created {track_name} with {} {} slices",
                boundaries.len() - 1,
                marker_name(markers),
            )),
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }
}

fn failure(message: &str) -> ArrangementAction {
    ArrangementAction {
        status: Some(message.into()),
        ..ArrangementAction::default()
    }
}

const fn marker_name(markers: AudioSliceMarkers) -> &'static str {
    match markers {
        AudioSliceMarkers::Transients => "Transient",
        AudioSliceMarkers::Warp => "Warp",
    }
}
