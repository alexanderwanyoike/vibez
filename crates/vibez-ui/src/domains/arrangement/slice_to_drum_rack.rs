//! Convert a marked Audio Clip into a native Drum Rack and reconstruction MIDI Clip.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};
use vibez_core::perform::GrooveGrid;

use crate::state::{
    ArrangementSelection, ProjectTrack, ProjectTracksState, TimelineEditorState, UiDrumPad,
    UiNoteClip,
};

use super::fragment_geometry::audio_fragment_geometry;
use super::slicing::marker_cut_positions;
use super::{attach_channel_eq, ArrangementAction, ArrangementCtx, AudioSliceMarkers};

const MAX_SLICES: usize = 16;
const FIRST_PAD_NOTE: u8 = 36;
const SLICE_VELOCITY: u8 = 100;

impl TimelineEditorState {
    pub(super) fn slice_audio_clip_to_drum_rack(
        &mut self,
        project_tracks: &mut ProjectTracksState,
        source_track_id: TrackId,
        source_clip_id: ClipId,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let Some(original) = self
            .find_content(source_track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == source_clip_id))
            .cloned()
        else {
            return ArrangementAction::default();
        };
        let Some(source) = original.source.clone() else {
            return failure("Slice to Drum Rack needs available Source Media");
        };
        if original.audio.num_frames() == 0 || ctx.samples_per_beat <= 0.0 {
            return failure("Slice to Drum Rack needs available audio and a valid tempo");
        }

        let transient_cuts = marker_cut_positions(&original, AudioSliceMarkers::Transients);
        let (mut cuts, marker_kind) = if transient_cuts.is_empty() {
            (
                marker_cut_positions(&original, AudioSliceMarkers::Warp),
                AudioSliceMarkers::Warp,
            )
        } else {
            (transient_cuts, AudioSliceMarkers::Transients)
        };
        if cuts.is_empty() {
            return failure("Add at least one interior Transient or Warp Marker first");
        }
        cuts.truncate(MAX_SLICES - 1);

        let mut boundaries = Vec::with_capacity(cuts.len() + 2);
        boundaries.push(0);
        boundaries.extend(cuts);
        boundaries.push(original.duration);
        let source_frames = original.audio.num_frames() as f32;
        let mut pads = Vec::with_capacity(boundaries.len() - 1);
        let mut notes = Vec::with_capacity(boundaries.len() - 1);
        for (index, boundary) in boundaries.windows(2).enumerate() {
            let local_start = boundary[0];
            let duration = boundary[1] - boundary[0];
            let (source_start, _, warp_markers) =
                audio_fragment_geometry(&original, local_start, duration);
            let source_end = warp_markers
                .source_end(source_start.saturating_add(duration))
                .min(original.audio.num_frames() as u64);
            if source_end <= source_start {
                return failure("A marker produced an empty Drum Rack slice");
            }
            pads.push(UiDrumPad {
                name: Some(format!("{} Slice {}", original.name, index + 1)),
                source: Some(source.clone()),
                audio: Some(Arc::clone(&original.audio)),
                gain: original.gain_db.linear().clamp(0.0, 2.0),
                pan: 0.0,
                start: source_start as f32 / source_frames,
                end: source_end as f32 / source_frames,
                coarse_tune: 0,
                fine_tune: 0.0,
                one_shot: true,
                choke_group: Some(1),
            });
            notes.push(MidiNote {
                pitch: FIRST_PAD_NOTE + index as u8,
                velocity: SLICE_VELOCITY,
                start_beat: local_start as f64 / ctx.samples_per_beat,
                duration_beats: duration as f64 / ctx.samples_per_beat,
            });
        }

        let track_number = project_tracks.next_unique_track_number("Slices");
        project_tracks.next_track_number = track_number + 1;
        let track_id = TrackId::new();
        let track_name = format!("Slices {track_number}");
        let color_index = (track_number.wrapping_sub(1) % 8) as u8;
        let mut track = ProjectTrack::new_instrument(
            track_id,
            track_name.clone(),
            TrackKind::Midi,
            color_index,
        );
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::DrumRack);
        for (slot, pad) in track.drum_rack_pads.iter_mut().zip(pads) {
            *slot = pad;
        }
        let mut no_engine = crate::domains::DiscardingEngine;
        attach_channel_eq(&mut no_engine, &mut track);

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

        project_tracks.tracks.push(track);
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
                marker_name(marker_kind)
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
