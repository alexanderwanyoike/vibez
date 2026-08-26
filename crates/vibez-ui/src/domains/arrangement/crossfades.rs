//! Explicit linked crossfades between two overlapping Audio Clips.

use vibez_core::id::{ClipId, TrackId};
use vibez_core::track::FadeCurve;
use vibez_engine::commands::EngineCommand;

use crate::state::{ArrangementSelection, AudioClipFadeEdge, TimelineEditorState};

use super::{ArrangementAction, EngineHandle};

impl TimelineEditorState {
    pub(super) fn crossfade_candidate_for_fade(
        &self,
        track_id: TrackId,
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
        frames: u64,
    ) -> Option<(ClipId, ClipId)> {
        let clips = &self.find_content(track_id)?.clips;
        let clip = clips.iter().find(|clip| clip.id == clip_id)?;
        let clip_end = clip.position.saturating_add(clip.duration);
        match edge {
            AudioClipFadeEdge::Out => clips
                .iter()
                .filter(|incoming| {
                    incoming.id != clip_id
                        && incoming.position > clip.position
                        && incoming.position < clip_end
                        && clip_end <= incoming.position.saturating_add(incoming.duration)
                        && clip_end.saturating_sub(incoming.position) == frames
                })
                .max_by_key(|incoming| incoming.position)
                .map(|incoming| (clip_id, incoming.id)),
            AudioClipFadeEdge::In => clips
                .iter()
                .filter(|outgoing| {
                    let outgoing_end = outgoing.position.saturating_add(outgoing.duration);
                    outgoing.id != clip_id
                        && outgoing.position < clip.position
                        && clip.position < outgoing_end
                        && outgoing_end <= clip_end
                        && outgoing_end.saturating_sub(clip.position) == frames
                })
                .min_by_key(|outgoing| outgoing.position.saturating_add(outgoing.duration))
                .map(|outgoing| (outgoing.id, clip_id)),
        }
    }

    pub(super) fn unlink_crossfade_edge_for_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
        edge: AudioClipFadeEdge,
    ) {
        let Some(content) = self.find_content_mut(track_id) else {
            return;
        };
        let Some(fades) = content
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.fades)
        else {
            return;
        };
        let peer = match edge {
            AudioClipFadeEdge::In => fades.crossfade_in_from(),
            AudioClipFadeEdge::Out => fades.crossfade_out_to(),
        };
        let mut changed = Vec::new();
        for clip in &mut content.clips {
            let next = if clip.id == clip_id {
                match edge {
                    AudioClipFadeEdge::In => clip.fades.unlink_fade_in(),
                    AudioClipFadeEdge::Out => clip.fades.unlink_fade_out(),
                }
            } else if peer == Some(clip.id) {
                match edge {
                    AudioClipFadeEdge::In if clip.fades.crossfade_out_to() == Some(clip_id) => {
                        clip.fades.unlink_fade_out()
                    }
                    AudioClipFadeEdge::Out if clip.fades.crossfade_in_from() == Some(clip_id) => {
                        clip.fades.unlink_fade_in()
                    }
                    _ => continue,
                }
            } else {
                continue;
            };
            if next != clip.fades {
                clip.fades = next;
                changed.push((clip.id, next));
            }
        }
        for (changed_id, fades) in changed {
            engine.send(EngineCommand::SetClipFades {
                track_id,
                clip_id: changed_id,
                fades,
            });
        }
    }

    /// Remove every link touching one Clip while retaining its audible fade
    /// lengths as ordinary linear fades. Geometry edits call this before they
    /// can make a persisted relationship stale.
    pub(super) fn unlink_crossfades_for_clip(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        clip_id: ClipId,
    ) {
        let Some(content) = self.find_content_mut(track_id) else {
            return;
        };
        let Some(fades) = content
            .clips
            .iter()
            .find(|clip| clip.id == clip_id)
            .map(|clip| clip.fades)
        else {
            return;
        };
        let incoming = fades.crossfade_in_from();
        let outgoing = fades.crossfade_out_to();
        let mut changed = Vec::new();
        for clip in &mut content.clips {
            let next = if clip.id == clip_id {
                clip.fades.unlinked()
            } else if incoming == Some(clip.id) && clip.fades.crossfade_out_to() == Some(clip_id) {
                clip.fades.unlink_fade_out()
            } else if outgoing == Some(clip.id) && clip.fades.crossfade_in_from() == Some(clip_id) {
                clip.fades.unlink_fade_in()
            } else {
                continue;
            };
            if next != clip.fades {
                clip.fades = next;
                changed.push((clip.id, next));
            }
        }
        for (changed_id, fades) in changed {
            engine.send(EngineCommand::SetClipFades {
                track_id,
                clip_id: changed_id,
                fades,
            });
        }
    }

    pub(super) fn crossfade_selected_audio_clips(
        &mut self,
        engine: &mut impl EngineHandle,
    ) -> ArrangementAction {
        let selected: Vec<_> = self
            .selected_clips
            .iter()
            .filter_map(|selection| match selection {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    Some((*track_id, *clip_id))
                }
                ArrangementSelection::NoteClip { .. } => None,
            })
            .collect();
        if self.selected_clips.len() != 2 || selected.len() != 2 || selected[0].0 != selected[1].0 {
            return ArrangementAction {
                status: Some("Select two overlapping Audio Clips on one Track".into()),
                ..ArrangementAction::default()
            };
        }
        let track_id = selected[0].0;
        let Some(content) = self.find_content(track_id) else {
            return ArrangementAction::default();
        };
        let mut clips: Vec<_> = selected
            .iter()
            .filter_map(|(_, id)| {
                content
                    .clips
                    .iter()
                    .find(|clip| clip.id == *id)
                    .map(|clip| {
                        (
                            clip.id,
                            clip.position,
                            clip.position.saturating_add(clip.duration),
                        )
                    })
            })
            .collect();
        clips.sort_by_key(|(_, start, _)| *start);
        let [(outgoing_id, outgoing_start, outgoing_end), (incoming_id, incoming_start, incoming_end)] =
            clips.as_slice()
        else {
            return ArrangementAction::default();
        };
        if outgoing_start == incoming_start
            || incoming_start >= outgoing_end
            || outgoing_end > incoming_end
        {
            return ArrangementAction {
                status: Some("The selected Clips need one shared edge overlap".into()),
                ..ArrangementAction::default()
            };
        }
        let outgoing_id = *outgoing_id;
        let incoming_id = *incoming_id;
        let overlap = outgoing_end - incoming_start;

        if !self.link_crossfade_pair(engine, track_id, outgoing_id, incoming_id, overlap) {
            return ArrangementAction::default();
        }

        ArrangementAction {
            status: Some("Created crossfade".into()),
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }

    pub(super) fn link_crossfade_pair(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        outgoing_id: ClipId,
        incoming_id: ClipId,
        overlap: u64,
    ) -> bool {
        self.unlink_crossfade_edge_for_clip(engine, track_id, outgoing_id, AudioClipFadeEdge::Out);
        self.unlink_crossfade_edge_for_clip(engine, track_id, incoming_id, AudioClipFadeEdge::In);
        let Some(content) = self.find_content_mut(track_id) else {
            return false;
        };
        let Some(outgoing_index) = content.clips.iter().position(|clip| clip.id == outgoing_id)
        else {
            return false;
        };
        let Some(incoming_index) = content.clips.iter().position(|clip| clip.id == incoming_id)
        else {
            return false;
        };
        let (outgoing, incoming) = if outgoing_index < incoming_index {
            let (left, right) = content.clips.split_at_mut(incoming_index);
            (&mut left[outgoing_index], &mut right[0])
        } else {
            let (left, right) = content.clips.split_at_mut(outgoing_index);
            (&mut right[0], &mut left[incoming_index])
        };
        outgoing.fades = outgoing
            .fades
            .linked_fade_out(overlap, incoming_id, outgoing.duration);
        incoming.fades = incoming
            .fades
            .linked_fade_in(overlap, outgoing_id, incoming.duration);
        let outgoing_fades = outgoing.fades;
        let incoming_fades = incoming.fades;
        engine.send(EngineCommand::SetClipFades {
            track_id,
            clip_id: outgoing_id,
            fades: outgoing_fades,
        });
        engine.send(EngineCommand::SetClipFades {
            track_id,
            clip_id: incoming_id,
            fades: incoming_fades,
        });
        true
    }

    pub(super) fn set_crossfade_curve(
        &mut self,
        engine: &mut impl EngineHandle,
        track_id: TrackId,
        outgoing_id: ClipId,
        incoming_id: ClipId,
        curve: FadeCurve,
    ) -> bool {
        let Some(content) = self.find_content_mut(track_id) else {
            return false;
        };
        let Some(outgoing_index) = content.clips.iter().position(|clip| clip.id == outgoing_id)
        else {
            return false;
        };
        let Some(incoming_index) = content.clips.iter().position(|clip| clip.id == incoming_id)
        else {
            return false;
        };
        let reciprocal = content.clips[outgoing_index].fades.crossfade_out_to()
            == Some(incoming_id)
            && content.clips[incoming_index].fades.crossfade_in_from() == Some(outgoing_id);
        if !reciprocal
            || (content.clips[outgoing_index].fades.fade_out_curve() == curve
                && content.clips[incoming_index].fades.fade_in_curve() == curve)
        {
            return false;
        }

        content.clips[outgoing_index].fades = content.clips[outgoing_index]
            .fades
            .with_linked_fade_out_curve(curve);
        content.clips[incoming_index].fades = content.clips[incoming_index]
            .fades
            .with_linked_fade_in_curve(curve);
        for index in [outgoing_index, incoming_index] {
            let clip = &content.clips[index];
            engine.send(EngineCommand::SetClipFades {
                track_id,
                clip_id: clip.id,
                fades: clip.fades,
            });
        }
        true
    }
}
