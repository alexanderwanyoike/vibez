//! Audio Clip fade edits shared by the timeline and Clip inspector.

use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::FadeCurve;
use vibez_engine::commands::EngineCommand;

use crate::state::{AudioClipFadeEdge, TimelineEditorState};

use super::{ArrangementAction, EngineHandle};

impl TimelineEditorState {
    pub(super) fn set_audio_clip_fade(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
        frames: u64,
    ) -> ArrangementAction {
        let changes_audible_fades = self
            .find_content(track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
            .is_some_and(|clip| {
                let next = match edge {
                    AudioClipFadeEdge::In => clip.fades.with_fade_in(frames, clip.duration),
                    AudioClipFadeEdge::Out => clip.fades.with_fade_out(frames, clip.duration),
                };
                next.fade_in_frames() != clip.fades.fade_in_frames()
                    || next.fade_out_frames() != clip.fades.fade_out_frames()
            });
        if !changes_audible_fades {
            return ArrangementAction::default();
        }

        self.unlink_crossfade_edge_for_clip(engine, track_id, clip_id, edge);
        let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
        else {
            return ArrangementAction::default();
        };
        clip.fades = match edge {
            AudioClipFadeEdge::In => clip.fades.with_fade_in(frames, clip.duration),
            AudioClipFadeEdge::Out => clip.fades.with_fade_out(frames, clip.duration),
        };
        engine.send(EngineCommand::SetClipFades {
            track_id,
            clip_id,
            fades: clip.fades,
        });
        self.discard_audio_clip_inspector_edits_for(clip_id);
        ArrangementAction {
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }

    pub(super) fn set_audio_clip_fade_curve(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
        curve: FadeCurve,
    ) -> ArrangementAction {
        let changes_curve = self
            .find_content(track_id)
            .and_then(|content| content.clips.iter().find(|clip| clip.id == clip_id))
            .is_some_and(|clip| match edge {
                AudioClipFadeEdge::In => {
                    clip.fades.fade_in_frames() > 0 && clip.fades.fade_in_curve() != curve
                }
                AudioClipFadeEdge::Out => {
                    clip.fades.fade_out_frames() > 0 && clip.fades.fade_out_curve() != curve
                }
            });
        if !changes_curve {
            return ArrangementAction::default();
        }

        self.unlink_crossfade_edge_for_clip(engine, track_id, clip_id, edge);
        let Some(clip) = self
            .find_content_mut(track_id)
            .and_then(|content| content.clips.iter_mut().find(|clip| clip.id == clip_id))
        else {
            return ArrangementAction::default();
        };
        clip.fades = match edge {
            AudioClipFadeEdge::In => clip.fades.with_fade_in_curve(curve),
            AudioClipFadeEdge::Out => clip.fades.with_fade_out_curve(curve),
        };
        engine.send(EngineCommand::SetClipFades {
            track_id,
            clip_id,
            fades: clip.fades,
        });
        ArrangementAction {
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }
}
