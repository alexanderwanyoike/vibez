//! Warp Marker edits for the shared Arrange and Section Audio Clip editor.

use vibez_engine::commands::EngineCommand;

use super::{ArrangementAction, ArrangementMsg, EngineHandle, TimelineEditorState};

impl TimelineEditorState {
    pub(super) fn update_warp_markers(
        &mut self,
        engine: &mut impl EngineHandle,
        message: ArrangementMsg,
    ) -> ArrangementAction {
        let mut action = ArrangementAction::default();
        match message {
            ArrangementMsg::SelectWarpMarker {
                track_id,
                clip_id,
                source_frame,
            } => {
                self.selected_warp_marker =
                    source_frame.map(|source_frame| (track_id, clip_id, source_frame));
                self.selected_transient_marker = None;
            }
            ArrangementMsg::AddWarpMarker {
                track_id,
                clip_id,
                source_frame,
                timeline_frame,
            } => {
                let markers = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        let source_end = clip.source_end();
                        let timeline_end = clip.warp_timeline_end();
                        clip.warp_markers
                            .add(
                                source_frame,
                                timeline_frame,
                                clip.source_offset,
                                source_end,
                                timeline_end,
                            )
                            .then(|| clip.warp_markers.clone())
                    });
                if let Some(warp_markers) = markers {
                    self.selected_warp_marker = Some((track_id, clip_id, source_frame));
                    self.selected_transient_marker = None;
                    engine.send(EngineCommand::SetClipWarpMarkers {
                        track_id,
                        clip_id,
                        warp_markers,
                    });
                    action.status = Some("Added Warp Marker".into());
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::MoveWarpMarker {
                track_id,
                clip_id,
                source_frame,
                timeline_frame,
            } => {
                let result = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        clip.warp_markers
                            .move_timeline(source_frame, timeline_frame)
                            .map(|moved_to| (moved_to, clip.warp_markers.clone()))
                    });
                if let Some((moved_to, warp_markers)) = result {
                    self.selected_warp_marker = Some((track_id, clip_id, source_frame));
                    engine.send(EngineCommand::SetClipWarpMarkers {
                        track_id,
                        clip_id,
                        warp_markers,
                    });
                    action.status = Some(format!("Warp Marker at frame {moved_to}"));
                    action.mark_dirty = true;
                }
            }
            ArrangementMsg::RemoveWarpMarker {
                track_id,
                clip_id,
                source_frame,
            } => {
                let markers = self
                    .find_content_mut(track_id)
                    .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
                    .and_then(|clip| {
                        clip.warp_markers
                            .remove(source_frame)
                            .then(|| clip.warp_markers.clone())
                    });
                if let Some(warp_markers) = markers {
                    self.selected_warp_marker = None;
                    engine.send(EngineCommand::SetClipWarpMarkers {
                        track_id,
                        clip_id,
                        warp_markers,
                    });
                    action.status = Some("Removed Warp Marker".into());
                    action.mark_dirty = true;
                }
            }
            _ => unreachable!("non-Warp Marker message reached its editor"),
        }
        action
    }
}
