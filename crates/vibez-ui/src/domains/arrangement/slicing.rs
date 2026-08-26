//! Marker-driven, nondestructive Audio Clip slicing.

use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::ClipPlaybackDirection;

use crate::state::{ArrangementSelection, TimelineEditorState, UiClip};

use super::fragment_geometry::audio_fragment;
use super::{ArrangementAction, AudioSliceMarkers, EngineHandle};

pub(super) const MAX_TIMELINE_SLICES: usize = 256;

impl TimelineEditorState {
    pub(super) fn slice_audio_clip_at_markers(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        markers: AudioSliceMarkers,
    ) -> ArrangementAction {
        let Some(original) = self
            .find_content(track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
            .cloned()
        else {
            return ArrangementAction::default();
        };
        let cuts = marker_cut_positions(&original, markers);
        if cuts.is_empty() {
            return ArrangementAction {
                status: Some(format!(
                    "No {} inside the visible Audio Clip",
                    marker_label(markers)
                )),
                ..ArrangementAction::default()
            };
        }

        let boundaries = slice_boundaries(&original, cuts);
        let slice_count = boundaries.len() - 1;
        if slice_count > MAX_TIMELINE_SLICES {
            return ArrangementAction {
                status: Some(format!(
                    "{} slice regions exceed the maximum of {MAX_TIMELINE_SLICES}",
                    slice_count
                )),
                ..ArrangementAction::default()
            };
        }
        let mut slices = Vec::with_capacity(boundaries.len() - 1);
        for (index, boundary) in boundaries.windows(2).enumerate() {
            let local_start = boundary[0];
            let duration = boundary[1] - boundary[0];
            let mut slice = audio_fragment(
                &original,
                format!("{} Slice {}", original.name, index + 1),
                local_start,
                duration,
            );
            // Slices are independent one-shots. Loop-wrap positions are added
            // to `slice_boundaries`, so disabling the inherited loop preserves
            // the exact audible sequence without leaving stale loop geometry.
            slice.loop_enabled = false;
            slice.reset_loop_region_to_clip();
            slices.push(slice);
        }

        let slice_ids: Vec<_> = slices.iter().map(|slice| slice.id).collect();
        self.replace_audio_clip(engine, track_id, clip_id, slices);
        self.selected_clips
            .remove(&ArrangementSelection::AudioClip { track_id, clip_id });
        self.selected_clips.extend(
            slice_ids
                .into_iter()
                .map(|clip_id| ArrangementSelection::AudioClip { track_id, clip_id }),
        );
        ArrangementAction {
            status: Some(format!(
                "Sliced Audio Clip into {} Clips at {}",
                boundaries.len() - 1,
                marker_label(markers)
            )),
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }
}

pub(crate) fn marker_cut_positions(clip: &UiClip, markers: AudioSliceMarkers) -> Vec<u64> {
    let source_frames: Vec<_> = match markers {
        AudioSliceMarkers::Transients => clip
            .transient_markers
            .as_slice()
            .iter()
            .map(|marker| marker.source_frame())
            .collect(),
        AudioSliceMarkers::Warp => clip
            .warp_markers
            .interior()
            .iter()
            .map(|marker| marker.source_frame())
            .collect(),
    };
    let timeline = clip.timeline();
    let mut cuts = Vec::new();
    for source_frame in source_frames {
        let timeline_source = if clip.warp_markers.is_empty() {
            source_frame
        } else {
            if source_frame < clip.source_offset || source_frame > clip.source_end() {
                continue;
            }
            clip.source_offset
                .saturating_add(clip.timeline_frame_at_source(source_frame))
        };
        cuts.extend(
            timeline
                .occurrences_of(timeline_source)
                .filter_map(|position| {
                    let cut = match clip.playback_direction {
                        ClipPlaybackDirection::Forward => position,
                        ClipPlaybackDirection::Reverse => {
                            clip.duration.saturating_sub(1).saturating_sub(position)
                        }
                    };
                    (cut > 0 && cut < clip.duration).then_some(cut)
                }),
        );
    }
    cuts.sort_unstable();
    cuts.dedup();
    cuts
}

/// Complete Clip-local boundaries for independently playable slices. Loop
/// wraps are boundaries even when no marker lands there, because a single
/// one-shot slice cannot represent a discontinuous source range.
pub(crate) fn slice_boundaries(clip: &UiClip, mut cuts: Vec<u64>) -> Vec<u64> {
    cuts.extend(clip.timeline().wraps().filter_map(|position| {
        let cut = match clip.playback_direction {
            ClipPlaybackDirection::Forward => position,
            ClipPlaybackDirection::Reverse => clip.duration.saturating_sub(position),
        };
        (cut > 0 && cut < clip.duration).then_some(cut)
    }));
    cuts.sort_unstable();
    cuts.dedup();

    let mut boundaries = Vec::with_capacity(cuts.len() + 2);
    boundaries.push(0);
    boundaries.extend(cuts);
    boundaries.push(clip.duration);
    boundaries
}

pub(crate) fn slice_region_count(clip: &UiClip, markers: AudioSliceMarkers) -> usize {
    let cuts = marker_cut_positions(clip, markers);
    if cuts.is_empty() {
        0
    } else {
        slice_boundaries(clip, cuts).len() - 1
    }
}

const fn marker_label(markers: AudioSliceMarkers) -> &'static str {
    match markers {
        AudioSliceMarkers::Transients => "Transient Markers",
        AudioSliceMarkers::Warp => "Warp Markers",
    }
}
