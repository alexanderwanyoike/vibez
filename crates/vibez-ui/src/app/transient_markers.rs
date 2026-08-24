//! Background Transient Marker detection and timeline result routing.

use std::sync::Arc;

use iced::Task;
use vibez_core::id::{ClipId, TrackId};

use crate::message::Message;

use super::*;

impl App {
    pub(super) fn dispatch_detect_clip_transients(
        &mut self,
        location: vibez_project::TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
    ) -> Task<Message> {
        let Some(content) = self.timeline_content_at(location, track_id) else {
            self.state.status_text = "Track not found".into();
            return Task::none();
        };
        let Some(clip) = content.clips.iter().find(|clip| clip.id == clip_id) else {
            self.state.status_text = "Clip not found".into();
            return Task::none();
        };
        let expected_audio = Arc::clone(&clip.audio);
        let detection_audio = Arc::clone(&expected_audio);
        self.state.status_text = format!("Detecting transients in {}...", clip.name);
        Task::perform(
            detect_clip_transients_async(detection_audio),
            move |source_frames| Message::ClipTransientsDetected {
                location,
                track_id,
                clip_id,
                expected_audio: Arc::clone(&expected_audio),
                source_frames,
            },
        )
    }

    pub(super) fn apply_detected_transients_at(
        &mut self,
        location: vibez_project::TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        source_frames: Vec<u64>,
    ) -> crate::domains::arrangement::ArrangementAction {
        let message =
            crate::domains::arrangement::ArrangementMsg::ReplaceDetectedTransientMarkers {
                track_id,
                clip_id,
                source_frames,
            };
        match location {
            vibez_project::TimelineLocation::Arrange => {
                let mut engine = crate::domains::DiscardingEngine;
                self.state.arrangement.update(
                    Arc::make_mut(&mut self.state.project_tracks),
                    message,
                    &mut engine,
                    Default::default(),
                )
            }
            vibez_project::TimelineLocation::Section(section_id)
                if self.state.perform.selected_section == Some(section_id) =>
            {
                let mut engine = crate::domains::DiscardingEngine;
                let action = self.state.perform.section_editor.editor_mut().update(
                    Arc::make_mut(&mut self.state.project_tracks),
                    message,
                    &mut engine,
                    Default::default(),
                );
                self.state.perform.commit_selected_section_timeline();
                if action.mark_dirty {
                    self.refresh_playing_section_after_edit(section_id);
                }
                action
            }
            vibez_project::TimelineLocation::Section(section_id) => {
                let action = {
                    let project_tracks = Arc::make_mut(&mut self.state.project_tracks);
                    let Some(section) =
                        Arc::make_mut(&mut self.state.perform.sections).by_id_mut(section_id)
                    else {
                        return Default::default();
                    };
                    let mut editor = crate::state::TimelineEditorState {
                        timeline: Arc::clone(&section.timeline),
                        ..Default::default()
                    };
                    let mut engine = crate::domains::DiscardingEngine;
                    let action =
                        editor.update(project_tracks, message, &mut engine, Default::default());
                    section.timeline = editor.timeline;
                    action
                };
                if action.mark_dirty {
                    self.refresh_playing_section_after_edit(section_id);
                }
                action
            }
        }
    }

    pub(super) fn finish_detect_clip_transients(
        &mut self,
        location: vibez_project::TimelineLocation,
        track_id: TrackId,
        clip_id: ClipId,
        expected_audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
        source_frames: Vec<u64>,
        undo_gesture: Option<crate::state::UndoGestureId>,
    ) -> Task<Message> {
        let still_current = self
            .timeline_content_at(location, track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
            .is_some_and(|clip| Arc::ptr_eq(&clip.audio, &expected_audio));
        if !still_current {
            self.state.status_text = "Transient detection ignored after Clip audio changed".into();
            return Task::none();
        }

        let snapshot = self.take_snapshot();
        let action = self.apply_detected_transients_at(location, track_id, clip_id, source_frames);
        if action.mark_dirty {
            self.state.project.history.push_edit(snapshot, undo_gesture);
            self.mark_project_dirty();
        }
        self.apply_arrangement_action_at(action, location)
    }
}
