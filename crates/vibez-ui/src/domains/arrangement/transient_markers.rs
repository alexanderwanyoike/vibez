//! Transient Marker edits for the shared Arrange and Section Clip editor.

use super::{ArrangementAction, ArrangementMsg, TimelineEditorState};
use crate::state::UiClip;
use vibez_core::transient::TransientMarkerKind;

fn source_bounds(clip: &UiClip) -> (u64, u64) {
    let audio_end = clip.audio.num_frames() as u64;
    let start = clip.source_offset.min(audio_end);
    let end = clip
        .source_offset
        .saturating_add(clip.duration)
        .min(audio_end)
        .max(start);
    (start, end)
}

impl TimelineEditorState {
    pub(super) fn update_transient_markers(
        &mut self,
        message: ArrangementMsg,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        match message {
            ArrangementMsg::SelectTransientMarker {
                track_id,
                clip_id,
                source_frame,
            } => {
                self.selected_transient_marker =
                    source_frame.map(|frame| (track_id, clip_id, frame));
            }
            ArrangementMsg::AddTransientMarker {
                track_id,
                clip_id,
                source_frame,
            } => {
                let added_frame = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        let (source_start, source_end) = source_bounds(clip);
                        let source_frame = source_frame.clamp(source_start, source_end);
                        clip.transient_markers
                            .add_authored(source_frame)
                            .then_some(source_frame)
                    });
                if let Some(source_frame) = added_frame {
                    self.selected_transient_marker = Some((track_id, clip_id, source_frame));
                    action.status = Some("Added Transient Marker".into());
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::MoveTransientMarker {
                track_id,
                clip_id,
                from,
                to,
            } => {
                let moved_to = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        let (source_start, source_end) = source_bounds(clip);
                        let to = to.clamp(source_start, source_end);
                        clip.transient_markers
                            .move_and_author(from, to)
                            .then_some(to)
                    });
                if let Some(to) = moved_to {
                    self.selected_transient_marker = Some((track_id, clip_id, to));
                    action.status = Some("Moved Transient Marker".into());
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::RemoveTransientMarker {
                track_id,
                clip_id,
                source_frame,
            } => {
                let removed = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .is_some_and(|clip| clip.transient_markers.remove(source_frame));
                if removed {
                    self.selected_transient_marker = None;
                    action.status = Some("Removed Transient Marker".into());
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::ReplaceDetectedTransientMarkers {
                track_id,
                clip_id,
                source_frames,
            } => {
                let result = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        let (source_start, source_end) = source_bounds(clip);
                        let before = clip.transient_markers.clone();
                        clip.transient_markers.replace_suggestions(
                            source_frames
                                .into_iter()
                                .filter(|frame| *frame >= source_start && *frame <= source_end),
                        );
                        (clip.transient_markers != before).then(|| {
                            clip.transient_markers
                                .as_slice()
                                .iter()
                                .filter(|marker| marker.kind() == TransientMarkerKind::Suggested)
                                .count()
                        })
                    });
                if let Some(count) = result {
                    self.selected_transient_marker = None;
                    action.status = Some(format!("Detected {count} Transient Markers"));
                    action.mark_dirty = true;
                }
            }
            _ => unreachable!("non-Transient Marker message reached its editor"),
        }
        action
    }
}
