//! Clip Projects own independent grid slots and a shared-editor selection.

use std::collections::HashMap;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_project::PerformLayout;

use crate::state::{
    ArrangementSelection, ArrangementTimeline, TimelineEditorState, UiClip, UiNoteClip,
};

use super::{PerformAction, PerformCtx, PerformEditorFocus, PerformState};

#[derive(Debug, Clone)]
pub struct LauncherClip {
    pub id: ClipId,
    pub track_id: TrackId,
    pub row: u32,
    pub timeline: Arc<ArrangementTimeline>,
}

#[derive(Debug, Clone, Copy)]
pub enum SlotPart<'a> {
    Audio(&'a UiClip),
    Midi(&'a UiNoteClip),
}

impl<'a> SlotPart<'a> {
    pub fn at(timeline: &'a ArrangementTimeline, track: TrackId) -> Option<Self> {
        let content = timeline.get(track)?;
        content
            .clips
            .first()
            .map(Self::Audio)
            .or_else(|| content.note_clips.first().map(Self::Midi))
    }

    pub fn looping(self) -> bool {
        match self {
            Self::Audio(clip) => clip.loop_enabled,
            Self::Midi(clip) => clip.loop_enabled,
        }
    }
}

impl LauncherClip {
    pub fn part(&self) -> Option<SlotPart<'_>> {
        SlotPart::at(&self.timeline, self.track_id)
    }

    pub fn name(&self) -> &str {
        match self.part() {
            Some(SlotPart::Audio(clip)) => &clip.name,
            Some(SlotPart::Midi(clip)) => &clip.name,
            None => "Empty clip",
        }
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
    pub pending: HashMap<u64, LauncherClip>,
    pub playing: HashMap<TrackId, LauncherClip>,
    pub queued: HashMap<TrackId, Option<ClipId>>,
    queued_requests: HashMap<TrackId, u64>,
    pub next_request: u64,
    pub running: bool,
    pub started_at: HashMap<TrackId, u64>,
    pub selected: Option<ClipId>,
    /// The cell paste targets: the selected clip's, or an empty one.
    pub slot: Option<(TrackId, u32)>,
    pub clipboard: Option<LauncherClip>,
    pub dragging: Option<ClipId>,
    pub editor: TimelineEditorState,
    pub first_track: usize,
    pub first_row: u32,
}

impl ClipEditor {
    pub fn reset_project(&mut self) {
        // Request IDs share the engine lifetime, which survives project changes.
        *self = Self {
            next_request: self.next_request,
            ..Self::default()
        };
    }

    pub fn resync_track(&mut self, state: vibez_engine::events::ClipTrackState, clips: &ClipStore) {
        let track = state.track_id;
        let playing = state.playing.and_then(|active| {
            self.pending
                .remove(&active.request_id)
                .filter(|clip| clip.id == active.clip_id)
                .or_else(|| {
                    self.playing
                        .get(&track)
                        .filter(|clip| clip.id == active.clip_id)
                        .cloned()
                })
                .or_else(|| clips.by_id(active.clip_id).cloned())
                .map(|clip| (clip, active.position))
        });
        if let Some((clip, position)) = playing {
            self.playing.insert(track, clip);
            self.started_at
                .insert(track, state.effective_at_samples.saturating_sub(position));
        } else {
            self.playing.remove(&track);
            self.started_at.remove(&track);
        }
        // A snapshot can arrive after the UI submits intent the engine has not consumed.
        if self
            .queued_requests
            .get(&track)
            .is_none_or(|request| *request <= state.through_request)
        {
            if let Some(queued) = state.queued {
                self.queue_request(track, queued.clip_id, queued.request_id);
            } else {
                self.queued.remove(&track);
                self.queued_requests.remove(&track);
            }
        }
        self.pending.retain(|request, clip| {
            clip.track_id != track
                || *request > state.through_request
                || state
                    .queued
                    .is_some_and(|queued| queued.request_id == *request)
        });
        if self.next_request <= state.through_request {
            self.running = state.running;
        }
    }

    pub fn playhead_samples(&self, id: ClipId, position: u64, spb: f64) -> Option<u64> {
        let clip = self.playing.values().find(|clip| clip.id == id)?;
        let start = *self.started_at.get(&clip.track_id)?;
        let (length, looping) = clip.length_and_loop(spb);
        let elapsed = position.saturating_sub(start);
        Some(if looping {
            elapsed % length
        } else {
            elapsed.min(length)
        })
    }

    pub fn progress(&self, id: ClipId, position: u64, spb: f64) -> Option<f32> {
        let clip = self.playing.values().find(|clip| clip.id == id)?;
        Some(self.playhead_samples(id, position, spb)? as f32 / clip.length_and_loop(spb).0 as f32)
    }

    pub fn queue_request(&mut self, track: TrackId, clip: Option<ClipId>, request: u64) {
        if self
            .queued_requests
            .get(&track)
            .is_none_or(|latest| request >= *latest)
        {
            self.queued_requests.insert(track, request);
            self.queued.insert(track, clip);
        }
    }

    pub fn acknowledge_transition(&mut self, track: TrackId, request: u64) {
        if request != 0
            && self
                .queued_requests
                .get(&track)
                .is_none_or(|latest| request >= *latest)
        {
            self.queued.remove(&track);
            self.queued_requests.remove(&track);
        }
    }

    pub fn clear_queue(&mut self) {
        self.queued.clear();
        self.queued_requests.clear();
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClipMsg {
    Toggle(ClipId),
    StopTrack(TrackId),
    LaunchRow(u32),
    StopAll,

    CreateMidi {
        track_id: TrackId,
        row: u32,
    },
    Select(ClipId),
    SelectSlot {
        track_id: TrackId,
        row: u32,
    },
    Delete(ClipId),
    ToggleLoop(ClipId),
    Duplicate(ClipId),
    Copy,
    Cut,
    Paste,
    BeginDrag(ClipId),
    EndDrag,
    /// `copy` leaves the dragged clip where it was.
    Drop {
        clip: ClipId,
        track_id: TrackId,
        row: u32,
        copy: bool,
    },
    MoveWindow {
        tracks: i32,
        rows: i32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipLaunchRequest {
    Clip(ClipId),
    Stop(TrackId),
    Row(u32),
    StopAll,
}

impl ClipMsg {
    pub const fn marks_dirty(&self) -> bool {
        matches!(
            self,
            Self::CreateMidi { .. } | Self::Delete(_) | Self::Duplicate(_) | Self::ToggleLoop(_)
        )
    }

    /// Edits that may change nothing: an empty clipboard, an empty cell, a
    /// track of the other kind. The app records them only once they have.
    pub const fn defers_project_edit(&self) -> bool {
        matches!(self, Self::Cut | Self::Paste | Self::Drop { .. })
    }
}

impl PerformState {
    pub fn toggle_clip_request(&self, id: ClipId) -> Option<ClipLaunchRequest> {
        let clip = self.clips.by_id(id)?;
        Some(if self.clip_is_triggered(clip) {
            ClipLaunchRequest::Stop(clip.track_id)
        } else {
            ClipLaunchRequest::Clip(id)
        })
    }

    pub fn clip_is_triggered(&self, clip: &LauncherClip) -> bool {
        match self.clip_editor.queued.get(&clip.track_id) {
            Some(queued) => *queued == Some(clip.id),
            None => self
                .clip_editor
                .playing
                .get(&clip.track_id)
                .is_some_and(|active| active.id == clip.id),
        }
    }

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
        self.clip_editor.slot = Some((clip.track_id, clip.row));
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

    /// Removes the clip, returning the stop its track needs when the clip
    /// was playing or about to.
    fn remove_launcher_clip(&mut self, id: ClipId) -> Option<ClipLaunchRequest> {
        let stop = self.clips.by_id(id).and_then(|clip| {
            (self
                .clip_editor
                .playing
                .get(&clip.track_id)
                .is_some_and(|active| active.id == id)
                || self.clip_editor.queued.get(&clip.track_id) == Some(&Some(id)))
            .then_some(ClipLaunchRequest::Stop(clip.track_id))
        });
        Arc::make_mut(&mut self.clips)
            .clips
            .retain(|clip| clip.id != id);
        if self.clip_editor.selected == Some(id) {
            self.clip_editor.selected = None;
            self.clip_editor.editor = Default::default();
        }
        stop
    }

    /// Empties the cell for `source` once its track can hold it, returning
    /// the stop the replaced clip needs, or why it cannot (`None` for a
    /// track that no longer exists).
    fn clear_cell_for(
        &mut self,
        source: &LauncherClip,
        track_id: TrackId,
        row: u32,
        tracks: &[crate::state::ProjectTrack],
    ) -> Result<Option<ClipLaunchRequest>, Option<&'static str>> {
        let track = tracks
            .iter()
            .find(|track| track.id == track_id)
            .ok_or(None)?;
        if let Some(status) = source.refused_by(track) {
            return Err(Some(status));
        }
        Ok(self
            .clips
            .at(track_id, row)
            .map(|clip| clip.id)
            .and_then(|occupant| self.remove_launcher_clip(occupant)))
    }

    pub(super) fn update_clips(&mut self, msg: ClipMsg, ctx: PerformCtx<'_>) -> PerformAction {
        if self.layout != PerformLayout::Clips || !ctx.workspace_visible {
            return PerformAction::default();
        }
        let mut selected = None;
        let mut clip_launch = None;
        match msg {
            ClipMsg::Toggle(id) => {
                return PerformAction {
                    clip_launch: self.toggle_clip_request(id),
                    ..Default::default()
                }
            }
            ClipMsg::StopTrack(id) => {
                return PerformAction {
                    clip_launch: Some(ClipLaunchRequest::Stop(id)),
                    ..Default::default()
                }
            }
            ClipMsg::LaunchRow(row) => {
                return PerformAction {
                    clip_launch: Some(ClipLaunchRequest::Row(row)),
                    ..Default::default()
                }
            }
            ClipMsg::StopAll => {
                return PerformAction {
                    clip_launch: Some(ClipLaunchRequest::StopAll),
                    ..Default::default()
                }
            }
            ClipMsg::ToggleLoop(id) => {
                if let Some(clip) = Arc::make_mut(&mut self.clips).by_id_mut(id) {
                    let part = SlotPart::at(&clip.timeline, clip.track_id);
                    let looping = part.is_none_or(SlotPart::looping);
                    let audio = matches!(part, Some(SlotPart::Audio(_)));
                    let content = Arc::make_mut(&mut clip.timeline).ensure(clip.track_id);
                    if audio {
                        if let Some(part) = content.clips.first_mut() {
                            part.loop_enabled = !looping;
                        }
                    } else if let Some(part) = content.note_clips.first_mut() {
                        part.loop_enabled = !looping;
                    }
                }
                if self.clip_editor.selected == Some(id) {
                    self.select_launcher_clip(id);
                }
            }
            ClipMsg::Select(id) => selected = self.select_launcher_clip(id),
            ClipMsg::BeginDrag(id) => self.clip_editor.dragging = Some(id),
            ClipMsg::EndDrag => self.clip_editor.dragging = None,
            ClipMsg::Drop {
                clip: id,
                track_id,
                row,
                copy,
            } => {
                self.clip_editor.dragging = None;
                let Some(source) = self.clips.by_id(id).cloned() else {
                    return PerformAction::default();
                };
                if (source.track_id, source.row) == (track_id, row) {
                    return PerformAction::default();
                }
                clip_launch = match self.clear_cell_for(&source, track_id, row, ctx.project_tracks)
                {
                    Ok(stop) => stop,
                    Err(status) => {
                        return PerformAction {
                            status,
                            ..Default::default()
                        }
                    }
                };
                let placed = if !copy && track_id == source.track_id {
                    if let Some(clip) = Arc::make_mut(&mut self.clips).by_id_mut(id) {
                        clip.row = row;
                    }
                    id
                } else {
                    let placed = source.copied_to(track_id, row);
                    let placed_id = placed.id;
                    Arc::make_mut(&mut self.clips).clips.push(placed);
                    if !copy {
                        clip_launch = self.remove_launcher_clip(id).or(clip_launch);
                    }
                    placed_id
                };
                selected = self.select_launcher_clip(placed);
            }
            ClipMsg::Copy => {
                self.clip_editor.clipboard = self
                    .clip_editor
                    .selected
                    .and_then(|id| self.clips.by_id(id))
                    .cloned();
            }
            ClipMsg::Cut => {
                let Some(id) = self.clip_editor.selected else {
                    return PerformAction::default();
                };
                self.clip_editor.clipboard = self.clips.by_id(id).cloned();
                clip_launch = self.remove_launcher_clip(id);
            }
            ClipMsg::Paste => {
                let Some((source, (track_id, row))) = self
                    .clip_editor
                    .clipboard
                    .clone()
                    .zip(self.clip_editor.slot)
                else {
                    return PerformAction::default();
                };
                clip_launch = match self.clear_cell_for(&source, track_id, row, ctx.project_tracks)
                {
                    Ok(stop) => stop,
                    Err(status) => {
                        return PerformAction {
                            status,
                            ..Default::default()
                        }
                    }
                };
                let copy = source.copied_to(track_id, row);
                let id = copy.id;
                Arc::make_mut(&mut self.clips).clips.push(copy);
                selected = self.select_launcher_clip(id);
            }
            ClipMsg::SelectSlot { track_id, row } => {
                if let Some(id) = self.clips.at(track_id, row).map(|clip| clip.id) {
                    selected = self.select_launcher_clip(id);
                } else {
                    self.clip_editor.selected = None;
                    self.clip_editor.slot = Some((track_id, row));
                    self.clip_editor.editor = Default::default();
                    return PerformAction {
                        select_project_track: Some(track_id),
                        ..Default::default()
                    };
                }
            }
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
                Arc::make_mut(&mut self.clips)
                    .clips
                    .push(super::clip_record::empty_midi_clip(
                        id,
                        track_id,
                        row,
                        format!("Pattern {}", row + 1),
                        16.0,
                    ));
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
                let copy = source.copied_to(track_id, row);
                let id = copy.id;
                Arc::make_mut(&mut self.clips).clips.push(copy);
                selected = self.select_launcher_clip(id);
                if row < self.clip_editor.first_row || row >= self.clip_editor.first_row + 4 {
                    self.clip_editor.first_row = row;
                }
            }
            ClipMsg::Delete(id) => clip_launch = self.remove_launcher_clip(id),
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
            clip_launch,
            select_project_track: selected,
            focus_clip_tab: selected.is_some(),
            ..Default::default()
        }
    }
}

impl LauncherClip {
    /// An independent copy in `track_id`'s `row`: its own id, so editing
    /// either never changes the other.
    pub fn copied_to(&self, track_id: TrackId, row: u32) -> LauncherClip {
        let id = ClipId::new();
        let mut timeline = (*self.timeline).clone();
        if track_id != self.track_id {
            if let Some(mut content) = timeline.by_track.remove(&self.track_id) {
                content
                    .automation
                    .retain(|lane| lane.target.on_every_track());
                timeline.by_track.insert(track_id, content);
            }
        }
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
        LauncherClip {
            id,
            track_id,
            row,
            timeline: Arc::new(timeline),
        }
    }

    /// Why `track` cannot hold this clip, if it cannot.
    pub fn refused_by(&self, track: &crate::state::ProjectTrack) -> Option<&'static str> {
        match self.part()? {
            SlotPart::Midi(_) if !track.kind.is_midi() => Some("A MIDI clip needs a MIDI track"),
            SlotPart::Audio(_) if track.kind.is_midi() => {
                Some("An audio clip needs an audio track")
            }
            _ => None,
        }
    }

    pub fn length_and_loop(&self, samples_per_beat: f64) -> (u64, bool) {
        match self.part() {
            Some(SlotPart::Audio(clip)) => (clip.duration.max(1), clip.loop_enabled),
            Some(SlotPart::Midi(clip)) => (
                (clip.duration_beats * samples_per_beat).round().max(1.0) as u64,
                clip.loop_enabled,
            ),
            None => (1, true),
        }
    }

    pub fn prepare(
        &self,
        request_id: u64,
        samples_per_beat: f64,
    ) -> Box<vibez_engine::playback_source::PreparedClipPlayback> {
        let (length_samples, looping) = self.length_and_loop(samples_per_beat);
        Box::new(vibez_engine::playback_source::PreparedClipPlayback {
            track_id: self.track_id,
            clip_id: Some(self.id),
            request_id,
            length_samples,
            looping,
            source: Box::new(super::sections::prepare_track_source(
                self.timeline.get(self.track_id),
            )),
        })
    }
}

#[cfg(test)]
#[path = "clip_launcher_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "clip_launcher_edit_tests.rs"]
mod edit_tests;
