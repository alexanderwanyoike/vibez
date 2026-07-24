//! Arrangement domain unit tests.

use super::*;
use crate::domains::test_support::RecordingEngine;
use crate::state::{
    new_master_track, ClipClipboard, ProjectTrack, ProjectTracksState, TrackTimelineContent, UiClip,
};
use vibez_core::automation::AutomationLane;

pub(super) struct TestTrack {
    project: ProjectTrack,
    pub(super) clips: Vec<UiClip>,
    pub(super) note_clips: Vec<UiNoteClip>,
    pub(super) automation: Vec<AutomationLane>,
}

impl TestTrack {
    fn from_parts(project: ProjectTrack, content: TrackTimelineContent) -> Self {
        Self {
            project,
            clips: content.clips,
            note_clips: content.note_clips,
            automation: content.automation,
        }
    }

    fn content(&self) -> TrackTimelineContent {
        TrackTimelineContent {
            clips: self.clips.clone(),
            note_clips: self.note_clips.clone(),
            automation: self.automation.clone(),
        }
    }
}

impl std::ops::Deref for TestTrack {
    type Target = ProjectTrack;

    fn deref(&self) -> &Self::Target {
        &self.project
    }
}

impl std::ops::DerefMut for TestTrack {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.project
    }
}

pub(super) struct ArrangementFixture {
    pub(super) arrangement: ArrangementState,
    pub(super) tracks: Vec<TestTrack>,
    pub(super) master: TestTrack,
    pub(super) buses: Vec<TestTrack>,
    pub(super) clipboard: ClipClipboard,
    next_track_number: u32,
}

impl Default for ArrangementFixture {
    fn default() -> Self {
        Self {
            arrangement: ArrangementState::default(),
            tracks: Vec::new(),
            master: TestTrack::from_parts(new_master_track(), TrackTimelineContent::default()),
            buses: Vec::new(),
            clipboard: ClipClipboard::default(),
            next_track_number: 1,
        }
    }
}

impl std::ops::Deref for ArrangementFixture {
    type Target = ArrangementState;

    fn deref(&self) -> &Self::Target {
        &self.arrangement
    }
}

impl std::ops::DerefMut for ArrangementFixture {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.arrangement
    }
}

impl ArrangementFixture {
    pub(super) fn update(
        &mut self,
        msg: ArrangementMsg,
        engine: &mut impl EngineHandle,
        ctx: ArrangementCtx,
    ) -> ArrangementAction {
        let mut project_tracks = ProjectTracksState {
            tracks: self
                .tracks
                .iter()
                .map(|track| track.project.clone())
                .collect(),
            master: self.master.project.clone(),
            buses: self
                .buses
                .iter()
                .map(|track| track.project.clone())
                .collect(),
            next_track_number: self.next_track_number,
        };
        let timeline = Arc::make_mut(&mut self.arrangement.timeline);
        for track in self
            .tracks
            .iter()
            .chain(self.buses.iter())
            .chain(std::iter::once(&self.master))
        {
            timeline.by_track.insert(track.id, track.content());
        }

        let action = if msg.is_clipboard_message() {
            self.arrangement.editor.update_clipboard(
                &project_tracks,
                msg,
                &mut self.clipboard,
                engine,
                ctx,
            )
        } else {
            self.arrangement
                .update(&mut project_tracks, msg, engine, ctx)
        };
        self.next_track_number = project_tracks.next_track_number;
        self.tracks = project_tracks
            .tracks
            .into_iter()
            .map(|track| {
                let content = self
                    .arrangement
                    .timeline
                    .get(track.id)
                    .cloned()
                    .unwrap_or_default();
                TestTrack::from_parts(track, content)
            })
            .collect();
        self.master = TestTrack::from_parts(
            project_tracks.master,
            self.arrangement
                .timeline
                .get(TrackId::MASTER)
                .cloned()
                .unwrap_or_default(),
        );
        self.buses = project_tracks
            .buses
            .into_iter()
            .map(|track| {
                let content = self
                    .arrangement
                    .timeline
                    .get(track.id)
                    .cloned()
                    .unwrap_or_default();
                TestTrack::from_parts(track, content)
            })
            .collect();
        action
    }

    pub(super) fn find_track(&self, track_id: TrackId) -> Option<&TestTrack> {
        if track_id.is_master() {
            return Some(&self.master);
        }
        self.tracks
            .iter()
            .chain(self.buses.iter())
            .find(|track| track.id == track_id)
    }
}

pub(super) fn arrangement_with_tracks(n: usize) -> ArrangementFixture {
    let mut a = ArrangementFixture::default();
    let mut engine = RecordingEngine::default();
    for _ in 0..n {
        a.update(
            ArrangementMsg::AddTrack,
            &mut engine,
            ArrangementCtx::default(),
        );
    }
    a
}
