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
        self.with_timeline_editor_at(location, move |editor, project_tracks, engine| {
            editor.update(project_tracks, message, engine, Default::default())
        })
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
