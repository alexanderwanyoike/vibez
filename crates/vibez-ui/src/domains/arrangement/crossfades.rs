//! Explicit linked crossfades between two overlapping Audio Clips.

use vibez_core::id::{ClipId, TrackId};
use vibez_engine::commands::EngineCommand;

use crate::state::{ArrangementSelection, TimelineEditorState};

use super::{ArrangementAction, EngineHandle};

impl TimelineEditorState {
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

        self.unlink_crossfades_for_clip(engine, track_id, outgoing_id);
        self.unlink_crossfades_for_clip(engine, track_id, incoming_id);
        let Some(content) = self.find_content_mut(track_id) else {
            return ArrangementAction::default();
        };
        let Some(outgoing_index) = content.clips.iter().position(|clip| clip.id == outgoing_id)
        else {
            return ArrangementAction::default();
        };
        let Some(incoming_index) = content.clips.iter().position(|clip| clip.id == incoming_id)
        else {
            return ArrangementAction::default();
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

        ArrangementAction {
            status: Some("Created equal-power crossfade".into()),
            mark_dirty: true,
            ..ArrangementAction::default()
        }
    }
}
