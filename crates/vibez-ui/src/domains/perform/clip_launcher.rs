//! Clip Projects own independent grid slots and a shared-editor selection.

use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_project::PerformLayout;

use crate::state::{ArrangementSelection, ArrangementTimeline, TimelineEditorState, UiNoteClip};

use super::{PerformAction, PerformCtx, PerformEditorFocus, PerformState};

#[derive(Debug, Clone)]
pub struct LauncherClip {
    pub id: ClipId,
    pub track_id: TrackId,
    pub row: u32,
    pub timeline: Arc<ArrangementTimeline>,
}

impl LauncherClip {
    pub fn name(&self) -> &str {
        self.timeline
            .get(self.track_id)
            .and_then(|content| {
                content
                    .clips
                    .first()
                    .map(|clip| clip.name.as_str())
                    .or_else(|| content.note_clips.first().map(|clip| clip.name.as_str()))
            })
            .unwrap_or("Empty clip")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ClipStore {
    pub clips: Vec<LauncherClip>,
}

impl ClipStore {
    pub fn at(&self, track_id: TrackId, row: u32) -> Option<&LauncherClip> {
        self.clips
            .iter()
            .find(|clip| clip.track_id == track_id && clip.row == row)
    }

    pub fn by_id(&self, id: ClipId) -> Option<&LauncherClip> {
        self.clips.iter().find(|clip| clip.id == id)
    }

    pub fn by_id_mut(&mut self, id: ClipId) -> Option<&mut LauncherClip> {
        self.clips.iter_mut().find(|clip| clip.id == id)
    }
}

#[derive(Debug, Default)]
pub struct ClipEditor {
    pub selected: Option<ClipId>,
    pub editor: TimelineEditorState,
    pub first_track: usize,
    pub first_row: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipMsg {
    CreateMidi { track_id: TrackId, row: u32 },
    Select(ClipId),
    Delete(ClipId),
    Duplicate(ClipId),
    MoveWindow { tracks: i32, rows: i32 },
}

impl ClipMsg {
    pub const fn marks_dirty(&self) -> bool {
        matches!(
            self,
            Self::CreateMidi { .. } | Self::Delete(_) | Self::Duplicate(_)
        )
    }
}

impl PerformState {
    pub fn selected_timeline_location(&self) -> Option<vibez_project::TimelineLocation> {
        match self.layout {
            PerformLayout::Sections => self
                .selected_section
                .map(vibez_project::TimelineLocation::Section),
            PerformLayout::Clips => self
                .clip_editor
                .selected
                .map(vibez_project::TimelineLocation::LauncherClip),
        }
    }

    pub fn has_selected_timeline(&self) -> bool {
        self.selected_timeline_location().is_some()
    }

    pub fn timeline_editor(&self) -> &TimelineEditorState {
        match self.layout {
            PerformLayout::Sections => self.section_editor.editor(),
            PerformLayout::Clips => &self.clip_editor.editor,
        }
    }

    pub fn timeline_editor_mut(&mut self) -> &mut TimelineEditorState {
        match self.layout {
            PerformLayout::Sections => self.section_editor.editor_mut(),
            PerformLayout::Clips => &mut self.clip_editor.editor,
        }
    }

    pub fn select_launcher_clip(&mut self, id: ClipId) -> Option<TrackId> {
        let clip = self.clips.by_id(id)?;
        self.clip_editor.selected = Some(id);
        let mut editor = TimelineEditorState {
            timeline: Arc::clone(&clip.timeline),
            selected_track: Some(clip.track_id),
            ..Default::default()
        };
        if let Some(content) = clip.timeline.get(clip.track_id) {
            if let Some(note) = content.note_clips.first() {
                editor.selected_note_clip = Some((clip.track_id, note.id));
            }
            if let Some(audio) = content.clips.first() {
                editor
                    .selected_clips
                    .insert(ArrangementSelection::AudioClip {
                        track_id: clip.track_id,
                        clip_id: audio.id,
                    });
            }
        }
        self.clip_editor.editor = editor;
        self.editor_focus = PerformEditorFocus::TimelineEditor;
        Some(clip.track_id)
    }

    pub(super) fn update_clips(&mut self, msg: ClipMsg, ctx: PerformCtx<'_>) -> PerformAction {
        if self.layout != PerformLayout::Clips || !ctx.workspace_visible {
            return PerformAction::default();
        }
        let mut selected = None;
        match msg {
            ClipMsg::Select(id) => selected = self.select_launcher_clip(id),
            ClipMsg::CreateMidi { track_id, row } => {
                if self.clips.at(track_id, row).is_some()
                    || !ctx
                        .project_tracks
                        .iter()
                        .any(|track| track.id == track_id && track.kind.is_midi())
                {
                    return PerformAction::default();
                }
                let id = ClipId::new();
                let mut timeline = ArrangementTimeline::default();
                timeline.ensure(track_id).note_clips.push(UiNoteClip {
                    id,
                    name: format!("Pattern {}", row + 1),
                    position_beats: 0.0,
                    duration_beats: 16.0,
                    notes: Vec::new(),
                    selected_notes: Default::default(),
                    start_marker_beats: 0.0,
                    loop_enabled: true,
                    loop_start_beats: 0.0,
                    loop_end_beats: 16.0,
                    groove_grid: Default::default(),
                });
                Arc::make_mut(&mut self.clips).clips.push(LauncherClip {
                    id,
                    track_id,
                    row,
                    timeline: Arc::new(timeline),
                });
                selected = self.select_launcher_clip(id);
            }
            ClipMsg::Duplicate(id) => {
                let Some(source) = self.clips.by_id(id) else {
                    return PerformAction::default();
                };
                let track_id = source.track_id;
                let Some(row) = (source.row.saturating_add(1)..u32::MAX - 8)
                    .find(|row| self.clips.at(track_id, *row).is_none())
                else {
                    return PerformAction::default();
                };
                let id = ClipId::new();
                let mut timeline = (*source.timeline).clone();
                for content in timeline.by_track.values_mut() {
                    for clip in &mut content.clips {
                        clip.id = id;
                        clip.fades = clip.fades.unlinked();
                    }
                    for clip in &mut content.note_clips {
                        clip.id = id;
                        clip.selected_notes.clear();
                    }
                    for lane in &mut content.automation {
                        lane.id = vibez_core::id::LaneId::new();
                    }
                }
                Arc::make_mut(&mut self.clips).clips.push(LauncherClip {
                    id,
                    track_id,
                    row,
                    timeline: Arc::new(timeline),
                });
                selected = self.select_launcher_clip(id);
                if row < self.clip_editor.first_row || row >= self.clip_editor.first_row + 4 {
                    self.clip_editor.first_row = row;
                }
            }
            ClipMsg::Delete(id) => {
                Arc::make_mut(&mut self.clips)
                    .clips
                    .retain(|clip| clip.id != id);
                if self.clip_editor.selected == Some(id) {
                    self.clip_editor.selected = None;
                    self.clip_editor.editor = Default::default();
                }
            }
            ClipMsg::MoveWindow { tracks, rows } => {
                self.clip_editor.first_track = self
                    .clip_editor
                    .first_track
                    .saturating_add_signed(tracks as isize)
                    .min(ctx.project_tracks.len().saturating_sub(4));
                self.clip_editor.first_row = self
                    .clip_editor
                    .first_row
                    .saturating_add_signed(rows)
                    .min(u32::MAX - 8);
            }
        }
        PerformAction {
            select_project_track: selected,
            focus_clip_tab: selected.is_some(),
            ..Default::default()
        }
    }
}

#[cfg(test)]
#[path = "clip_launcher_tests.rs"]
mod tests;
