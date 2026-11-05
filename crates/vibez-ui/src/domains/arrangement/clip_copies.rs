//! Copies of selected Clips, collected with their lane offsets and placed
//! at a new start over whatever lies there.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;
use vibez_engine::commands::EngineCommand;

use super::EngineHandle;
use crate::state::{ArrangementSelection, ClipboardClip, ProjectTracksState, TimelineEditorState};

impl ClipboardClip {
    pub(super) fn source_track(&self) -> TrackId {
        match self {
            Self::Audio { track_id, .. } | Self::Note { track_id, .. } => *track_id,
        }
    }

    fn set_track_offset(&mut self, offset: usize) {
        match self {
            Self::Audio { track_offset, .. } | Self::Note { track_offset, .. } => {
                *track_offset = offset;
            }
        }
    }

    pub(super) fn track_offset(&self) -> usize {
        match self {
            Self::Audio { track_offset, .. } | Self::Note { track_offset, .. } => *track_offset,
        }
    }

    pub(super) fn position_beats(&self) -> f64 {
        match self {
            Self::Audio { position_beats, .. } | Self::Note { position_beats, .. } => {
                *position_beats
            }
        }
    }
}

impl TimelineEditorState {
    /// The time selection's content on the lanes `in_range` accepts when one
    /// is active, otherwise the selected clips, with lane offsets relative to
    /// the topmost source lane.
    pub(super) fn collect_selected_clips(
        &self,
        project_tracks: &ProjectTracksState,
        spb: f64,
        in_range: impl Fn(TrackId) -> bool,
    ) -> Vec<ClipboardClip> {
        let mut copied = Vec::new();

        if self.time_selection_active
            && self.selection_end_beats > self.selection_start_beats
            && spb > 0.0
        {
            let start = self.selection_start_beats;
            let end = self.selection_end_beats;
            for (track_id, content) in self
                .timeline
                .by_track
                .iter()
                .filter(|(track_id, _)| in_range(**track_id))
            {
                for clip in &content.clips {
                    let clip_start = clip.position as f64 / spb;
                    let clip_end = (clip.position + clip.duration) as f64 / spb;
                    let overlap_start = clip_start.max(start);
                    let overlap_end = clip_end.min(end);
                    if overlap_end <= overlap_start {
                        continue;
                    }
                    let delta = ((overlap_start - clip_start) * spb).round() as u64;
                    let duration = ((overlap_end - overlap_start) * spb).round() as u64;
                    let mut fragment = clip.clone();
                    fragment.fades = fragment.fades.unlinked();
                    fragment.position = 0;
                    fragment.duration = duration.max(1);
                    fragment.fades =
                        clip.fades
                            .for_fragment(clip.duration, delta, fragment.duration);
                    (
                        fragment.source_offset,
                        fragment.start_marker,
                        fragment.warp_markers,
                    ) = clip.warp_geometry_for_fragment(delta, fragment.duration);
                    copied.push(ClipboardClip::Audio {
                        track_id: *track_id,
                        track_offset: 0,
                        position_beats: overlap_start,
                        clip: fragment,
                    });
                }

                for clip in &content.note_clips {
                    let clip_end = clip.position_beats + clip.duration_beats;
                    let overlap_start = clip.position_beats.max(start);
                    let overlap_end = clip_end.min(end);
                    if overlap_end <= overlap_start {
                        continue;
                    }
                    let local_start = overlap_start - clip.position_beats;
                    let local_end = overlap_end - clip.position_beats;
                    let mut notes = Vec::new();
                    for note in &clip.notes {
                        for occurrence in clip.note_occurrences(note.start_beat) {
                            let kept_start = occurrence.max(local_start);
                            let kept_end = (occurrence + note.duration_beats).min(local_end);
                            if kept_end > kept_start {
                                notes.push(MidiNote {
                                    start_beat: kept_start - local_start,
                                    duration_beats: kept_end - kept_start,
                                    ..*note
                                });
                            }
                        }
                    }
                    let mut fragment = clip.clone();
                    fragment.position_beats = 0.0;
                    fragment.duration_beats = overlap_end - overlap_start;
                    fragment.notes = notes;
                    fragment.selected_notes.clear();
                    fragment.start_marker_beats = 0.0;
                    fragment.loop_enabled = false;
                    fragment.loop_start_beats = 0.0;
                    fragment.loop_end_beats = 0.0;
                    copied.push(ClipboardClip::Note {
                        track_id: *track_id,
                        track_offset: 0,
                        position_beats: overlap_start,
                        clip: fragment,
                    });
                }
            }
        } else {
            for selection in &self.selected_clips {
                match selection {
                    ArrangementSelection::AudioClip { track_id, clip_id } if spb > 0.0 => {
                        if let Some(clip) = self.find_content(*track_id).and_then(|content| {
                            content.clips.iter().find(|clip| clip.id == *clip_id)
                        }) {
                            let mut copied_clip = clip.clone();
                            copied_clip.fades = copied_clip.fades.unlinked();
                            copied.push(ClipboardClip::Audio {
                                track_id: *track_id,
                                track_offset: 0,
                                position_beats: clip.position as f64 / spb,
                                clip: copied_clip,
                            });
                        }
                    }
                    ArrangementSelection::NoteClip { track_id, clip_id } => {
                        if let Some(clip) = self.find_content(*track_id).and_then(|content| {
                            content.note_clips.iter().find(|clip| clip.id == *clip_id)
                        }) {
                            copied.push(ClipboardClip::Note {
                                track_id: *track_id,
                                track_offset: 0,
                                position_beats: clip.position_beats,
                                clip: clip.clone(),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }

        let top_index = copied
            .iter()
            .filter_map(|entry| {
                project_tracks
                    .tracks
                    .iter()
                    .position(|track| track.id == entry.source_track())
            })
            .min();
        if let Some(top_index) = top_index {
            for entry in &mut copied {
                if let Some(index) = project_tracks
                    .tracks
                    .iter()
                    .position(|track| track.id == entry.source_track())
                {
                    entry.set_track_offset(index - top_index);
                }
            }
        }
        copied
    }

    /// Adds each clip to its lane so that `origin_beats` lands on
    /// `anchor_beats`, overwriting what it lands on, and returns the new
    /// clips as a selection.
    pub(super) fn place_clips(
        &mut self,
        entries: impl IntoIterator<Item = (ClipboardClip, TrackId)>,
        origin_beats: f64,
        anchor_beats: f64,
        engine: &mut impl EngineHandle,
        spb: f64,
    ) -> HashSet<ArrangementSelection> {
        let mut selected = HashSet::new();
        for (entry, track_id) in entries {
            let position_beats = anchor_beats + (entry.position_beats() - origin_beats);
            match entry {
                ClipboardClip::Audio { mut clip, .. } => {
                    clip.fades = clip.fades.unlinked();
                    clip.id = ClipId::new();
                    clip.position = (position_beats * spb).round().max(0.0) as u64;
                    engine.send(EngineCommand::AddClip {
                        insertion: vibez_engine::clip_insertion::PreparedClipInsertion::new(),
                        track_id,
                        clip_id: clip.id,
                        audio: Arc::clone(&clip.audio),
                        position: clip.position,
                        source_offset: clip.source_offset,
                        start_marker: clip.start_marker,
                        duration: clip.duration,
                        loop_enabled: clip.loop_enabled,
                        loop_start: clip.loop_start,
                        loop_end: clip.loop_end,
                        linear_gain: clip.gain_db.linear(),
                        fades: clip.fades,
                        playback_direction: clip.playback_direction,
                        warp_markers: clip.warp_markers.clone(),
                    });
                    selected.insert(ArrangementSelection::AudioClip {
                        track_id,
                        clip_id: clip.id,
                    });
                    Arc::make_mut(&mut self.timeline)
                        .ensure(track_id)
                        .clips
                        .push(clip);
                }
                ClipboardClip::Note { mut clip, .. } => {
                    clip.id = ClipId::new();
                    clip.position_beats = position_beats;
                    clip.selected_notes.clear();
                    engine.send(clip.add_command(track_id));
                    selected.insert(ArrangementSelection::NoteClip {
                        track_id,
                        clip_id: clip.id,
                    });
                    Arc::make_mut(&mut self.timeline)
                        .ensure(track_id)
                        .note_clips
                        .push(clip);
                }
            }
        }
        let placed: Vec<_> = selected.iter().copied().collect();
        self.overwrite_under(engine, &placed, spb);
        selected
    }
}
