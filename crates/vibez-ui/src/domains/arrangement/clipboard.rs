//! One clip clipboard shared by Arrange and every Section timeline.

use vibez_engine::commands::EngineCommand;

use super::{ArrangementAction, ArrangementCtx, ArrangementMsg, EngineHandle};
use crate::state::{
    ArrangementSelection, ClipClipboard, ClipboardClip, ProjectTracksState, TimelineEditorState,
};

impl TimelineEditorState {
    pub(crate) fn update_clipboard(
        &mut self,
        project_tracks: &ProjectTracksState,
        msg: ArrangementMsg,
        clipboard: &mut ClipClipboard,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        match msg {
            ArrangementMsg::CopySelectedClips => {
                self.op_copy_selected_clips(project_tracks, clipboard, ctx)
            }
            ArrangementMsg::CutSelectedClips => {
                let start = self.selection_start_beats;
                let end = self.selection_end_beats;
                let track_id = self.time_selection_track;
                let ranged = self.time_selection_active && end > start;
                let copy = self.op_copy_selected_clips(project_tracks, clipboard, ctx);
                if copy.status.as_deref() == Some("Nothing to copy") {
                    return copy;
                }

                let mut action = if ranged {
                    self.op_delete_clips_in_region(engine, ctx, start, end, track_id)
                } else {
                    self.remove_selected_clips(engine)
                };
                action.status = Some("Cut to clipboard".to_string());
                action.mark_dirty = true;
                action
            }
            ArrangementMsg::PasteClips => {
                self.op_paste_clips(project_tracks, clipboard, engine, ctx)
            }
            _ => unreachable!("only clipboard messages enter the clipboard boundary"),
        }
    }

    fn remove_selected_clips(&mut self, engine: &mut impl EngineHandle) -> ArrangementAction {
        let selections: Vec<_> = self.selected_clips.drain().collect();
        for selection in &selections {
            match selection {
                ArrangementSelection::AudioClip { track_id, clip_id } => {
                    self.unlink_crossfades_for_clip(engine, *track_id, *clip_id);
                    engine.send(EngineCommand::RemoveClip(*track_id, *clip_id));
                    if let Some(track) = self.find_content_mut(*track_id) {
                        track.clips.retain(|clip| clip.id != *clip_id);
                    }
                }
                ArrangementSelection::NoteClip { track_id, clip_id } => {
                    engine.send(EngineCommand::RemoveNoteClip(*track_id, *clip_id));
                    if let Some(track) = self.find_content_mut(*track_id) {
                        track.note_clips.retain(|clip| clip.id != *clip_id);
                    }
                    if self
                        .selected_note_clip
                        .is_some_and(|selected| selected == (*track_id, *clip_id))
                    {
                        self.selected_note_clip = None;
                    }
                }
            }
        }
        ArrangementAction::default()
    }

    fn op_copy_selected_clips(
        &self,
        project_tracks: &ProjectTracksState,
        clipboard: &mut ClipClipboard,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let copied = self.collect_selected_clips(project_tracks, ctx.samples_per_beat, |lane| {
            self.time_selection_track
                .is_none_or(|target| lane == target)
        });
        let count = copied.len();
        if count > 0 {
            *clipboard = ClipClipboard { clips: copied };
        }
        ArrangementAction {
            status: Some(match count {
                0 => "Nothing to copy".to_string(),
                1 => "Copied clip".to_string(),
                _ => format!("Copied {count} clips"),
            }),
            ..Default::default()
        }
    }

    fn op_paste_clips(
        &mut self,
        project_tracks: &ProjectTracksState,
        clipboard: &ClipClipboard,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        if clipboard.clips.is_empty() {
            return ArrangementAction {
                status: Some("Clipboard is empty".to_string()),
                ..Default::default()
            };
        }
        if ctx.samples_per_beat <= 0.0 {
            return ArrangementAction {
                status: Some("Can't paste clips without a valid project tempo".to_string()),
                ..Default::default()
            };
        }
        let Some(anchor_track) = self.selected_track else {
            return ArrangementAction {
                status: Some("Select a destination Project Track".to_string()),
                ..Default::default()
            };
        };
        let Some(anchor_index) = project_tracks
            .tracks
            .iter()
            .position(|track| track.id == anchor_track)
        else {
            return ArrangementAction {
                status: Some("Select a destination Project Track".to_string()),
                ..Default::default()
            };
        };

        let mut destinations = Vec::with_capacity(clipboard.clips.len());
        for entry in &clipboard.clips {
            let Some(index) = anchor_index.checked_add(entry.track_offset()) else {
                return incompatible_paste("the Track layout runs past the project");
            };
            let Some(track) = project_tracks.tracks.get(index) else {
                return incompatible_paste("the Track layout runs past the project");
            };
            let compatible = match entry {
                ClipboardClip::Audio { .. } => !track.kind.is_midi(),
                ClipboardClip::Note { .. } => track.kind.is_midi(),
            };
            if !compatible {
                return incompatible_paste(&format!("{} has the wrong Clip type", track.name));
            }
            destinations.push(track.id);
        }

        let earliest_position_beats = clipboard
            .clips
            .iter()
            .map(ClipboardClip::position_beats)
            .min_by(f64::total_cmp)
            .unwrap_or(0.0);
        let selected = self.place_clips(
            clipboard.clips.iter().cloned().zip(destinations),
            earliest_position_beats,
            ctx.playhead_beats.max(0.0),
            engine,
            ctx.samples_per_beat,
        );

        let count = selected.len();
        self.selected_clips = selected;
        self.selected_track = Some(anchor_track);
        self.clear_time_selection();
        ArrangementAction {
            status: Some(if count == 1 {
                "Pasted clip".to_string()
            } else {
                format!("Pasted {count} clips")
            }),
            mark_dirty: true,
            ..Default::default()
        }
    }
}

fn incompatible_paste(reason: &str) -> ArrangementAction {
    ArrangementAction {
        status: Some(format!("Can't paste clips: {reason}")),
        ..Default::default()
    }
}
