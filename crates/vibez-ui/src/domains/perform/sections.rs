use std::sync::Arc;

use vibez_core::id::{ClipId, LaneId, SectionId, TrackId};
use vibez_engine::playback_source::{
    EngineClip, EngineNoteClip, PreparedPlaybackSource, PreparedSectionPlaybackSource,
};
use vibez_project::SectionLaunchQuantization;

use crate::domains::timeline_editor::TimelineEditorAdapter;
use crate::state::{
    ArrangementTimeline, ResolvedTimeline, ResolvedTimelineMut, TimelineEditorState,
};

pub const DEFAULT_SECTION_LENGTH_BEATS: f64 = 16.0;
pub const MIN_SECTION_LENGTH_BEATS: f64 = 4.0;
pub const MAX_SECTION_LENGTH_BEATS: f64 = 1024.0;

/// Runtime-only editor adapter for the selected Section.
///
/// Persisted Section content remains in [`Section::timeline`]. Selection,
/// clipboard, and pointer interaction state stay outside the canonical Section
/// store and are reset when a different Section is selected.
#[derive(Debug, Default)]
pub struct SectionTimelineEditor {
    editor: TimelineEditorState,
}

impl SectionTimelineEditor {
    pub fn load(&mut self, timeline: Arc<ArrangementTimeline>, selected_track: Option<TrackId>) {
        self.editor = TimelineEditorState {
            timeline,
            selected_track,
            ..TimelineEditorState::default()
        };
    }

    pub fn clear(&mut self) {
        self.editor = TimelineEditorState::default();
    }

    pub fn editor(&self) -> &TimelineEditorState {
        &self.editor
    }

    pub fn editor_mut(&mut self) -> &mut TimelineEditorState {
        &mut self.editor
    }
}

impl TimelineEditorAdapter for SectionTimelineEditor {
    fn resolve_timeline(&self) -> ResolvedTimeline<'_> {
        ResolvedTimeline {
            editor: &self.editor,
        }
    }

    fn resolve_timeline_mut(&mut self) -> ResolvedTimelineMut<'_> {
        ResolvedTimelineMut {
            editor: &mut self.editor,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Section {
    pub id: SectionId,
    pub slot: u16,
    pub name: String,
    pub length_beats: f64,
    pub launch_quantization: SectionLaunchQuantization,
    pub looping: bool,
    pub timeline: Arc<ArrangementTimeline>,
}

impl Section {
    pub fn new(slot: u16) -> Self {
        Self {
            id: SectionId::new(),
            slot,
            name: format!("Section {:02}", slot + 1),
            length_beats: DEFAULT_SECTION_LENGTH_BEATS,
            launch_quantization: SectionLaunchQuantization::default(),
            looping: true,
            timeline: Arc::new(ArrangementTimeline::default()),
        }
    }

    pub fn duplicate_to(&self, slot: u16) -> Self {
        let mut timeline = self.timeline.as_ref().clone();
        for content in timeline.by_track.values_mut() {
            for clip in &mut content.clips {
                clip.id = ClipId::new();
            }
            for clip in &mut content.note_clips {
                clip.id = ClipId::new();
                clip.selected_notes.clear();
            }
            for lane in &mut content.automation {
                lane.id = LaneId::new();
            }
        }
        Self {
            id: SectionId::new(),
            slot,
            name: format!("{} Copy", self.name),
            length_beats: self.length_beats,
            launch_quantization: self.launch_quantization,
            looping: self.looping,
            timeline: Arc::new(timeline),
        }
    }

    /// Whether a beat is active inside this Section's non-destructive
    /// playable boundary. Timeline content outside the boundary remains stored.
    pub fn contains_playable_beat(&self, beat: f64) -> bool {
        beat >= 0.0 && beat < self.length_beats
    }

    /// Resolve this Section into the engine's resident playback-source seam.
    /// Every Project Track receives a source, including empty sources that
    /// express intentional silence without resetting the shared channel.
    pub fn prepare_playback_source(
        &self,
        project_tracks: &[crate::state::ProjectTrack],
    ) -> PreparedSectionPlaybackSource {
        let track_ids: Vec<_> = project_tracks.iter().map(|track| track.id).collect();
        self.prepare_playback_source_for_tracks(&track_ids)
    }

    pub fn prepare_playback_source_for_tracks(
        &self,
        project_track_ids: &[TrackId],
    ) -> PreparedSectionPlaybackSource {
        let tracks = project_track_ids
            .iter()
            .map(|track_id| {
                let content = self.timeline.get(*track_id);
                let clips = content
                    .into_iter()
                    .flat_map(|content| content.clips.iter())
                    .map(|clip| EngineClip {
                        id: clip.id,
                        audio: Arc::clone(&clip.audio),
                        position: clip.position,
                        source_offset: clip.source_offset,
                        duration: clip.duration,
                        loop_enabled: clip.loop_enabled,
                        loop_start: clip.loop_start,
                        loop_end: clip.loop_end,
                    })
                    .collect();
                let note_clips = content
                    .into_iter()
                    .flat_map(|content| content.note_clips.iter())
                    .map(|clip| EngineNoteClip {
                        id: clip.id,
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        notes: clip.notes.clone(),
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
                    })
                    .collect();
                let automation = content
                    .map(|content| content.automation.clone())
                    .unwrap_or_default();
                (
                    *track_id,
                    PreparedPlaybackSource::new(clips, note_clips, automation),
                )
            })
            .collect();
        PreparedSectionPlaybackSource::new(self.id, self.length_beats, self.looping, tracks)
    }
}

#[derive(Debug, Clone, Default)]
pub struct SectionStore {
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineContentLocation {
    Arrange,
    Section { slot: u16, name: String },
}

impl SectionStore {
    pub fn at_slot(&self, slot: u16) -> Option<&Section> {
        self.sections.iter().find(|section| section.slot == slot)
    }

    pub fn by_id(&self, id: SectionId) -> Option<&Section> {
        self.sections.iter().find(|section| section.id == id)
    }

    pub fn by_id_mut(&mut self, id: SectionId) -> Option<&mut Section> {
        self.sections.iter_mut().find(|section| section.id == id)
    }

    pub fn insert(&mut self, section: Section) {
        debug_assert!(self.at_slot(section.slot).is_none());
        self.sections.push(section);
        self.sections.sort_by_key(|section| section.slot);
    }

    pub fn remove(&mut self, id: SectionId) -> Option<Section> {
        let index = self.sections.iter().position(|section| section.id == id)?;
        Some(self.sections.remove(index))
    }

    pub fn remove_track(&mut self, track_id: TrackId) {
        for section in &mut self.sections {
            Arc::make_mut(&mut section.timeline).remove(track_id);
        }
    }

    pub fn track_content_locations(
        &self,
        arrangement: &ArrangementTimeline,
        track_id: TrackId,
    ) -> Vec<TimelineContentLocation> {
        let has_content = |timeline: &ArrangementTimeline| {
            timeline.get(track_id).is_some_and(|content| {
                !content.clips.is_empty()
                    || !content.note_clips.is_empty()
                    || !content.automation.is_empty()
            })
        };
        let mut locations = Vec::new();
        if has_content(arrangement) {
            locations.push(TimelineContentLocation::Arrange);
        }
        locations.extend(
            self.sections
                .iter()
                .filter(|section| has_content(&section.timeline))
                .map(|section| TimelineContentLocation::Section {
                    slot: section.slot,
                    name: section.name.clone(),
                }),
        );
        locations
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::timeline_editor::conformance::assert_timeline_editor_conformance;
    use crate::state::{ArrangementState, TrackTimelineContent, UiNoteClip};

    #[test]
    fn duplicate_remints_every_editable_identity() {
        let track_id = TrackId::new();
        let mut source = Section::new(0);
        let note_id = ClipId::new();
        let lane = vibez_core::automation::AutomationLane::new(
            vibez_core::automation::AutomationTarget::TrackGain,
        );
        let original_lane_id = lane.id;
        Arc::make_mut(&mut source.timeline).by_track.insert(
            track_id,
            TrackTimelineContent {
                note_clips: vec![UiNoteClip {
                    id: note_id,
                    name: "Pattern".into(),
                    position_beats: 0.0,
                    duration_beats: 4.0,
                    notes: Vec::new(),
                    selected_notes: [0].into_iter().collect(),
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                }],
                automation: vec![lane],
                ..TrackTimelineContent::default()
            },
        );

        let duplicate = source.duplicate_to(7);

        assert_ne!(duplicate.id, source.id);
        assert_ne!(
            duplicate.timeline.get(track_id).unwrap().note_clips[0].id,
            note_id
        );
        assert_ne!(
            duplicate.timeline.get(track_id).unwrap().automation[0].id,
            original_lane_id
        );
        assert!(duplicate.timeline.get(track_id).unwrap().note_clips[0]
            .selected_notes
            .is_empty());
    }

    #[test]
    fn section_adapter_satisfies_the_shared_editor_contract() {
        assert_timeline_editor_conformance(SectionTimelineEditor::default());
    }

    #[test]
    fn arrange_and_section_adapters_never_share_timeline_mutations() {
        let track_id = TrackId::new();
        let mut arrange = ArrangementState::default();
        let mut section = SectionTimelineEditor::default();
        section.load(Arc::new(ArrangementTimeline::default()), Some(track_id));

        Arc::make_mut(&mut section.editor_mut().timeline)
            .ensure(track_id)
            .note_clips
            .push(UiNoteClip {
                id: ClipId::new(),
                name: "Section Pattern".into(),
                position_beats: 0.0,
                duration_beats: 4.0,
                notes: Vec::new(),
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            });

        assert!(arrange.editor.timeline.get(track_id).is_none());
        assert_eq!(
            section
                .editor()
                .timeline
                .get(track_id)
                .expect("Section track content")
                .note_clips
                .len(),
            1
        );

        Arc::make_mut(&mut arrange.editor.timeline)
            .ensure(track_id)
            .note_clips
            .push(UiNoteClip {
                id: ClipId::new(),
                name: "Arrange Pattern".into(),
                position_beats: 8.0,
                duration_beats: 4.0,
                notes: Vec::new(),
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            });
        let section_clip = &section.editor().timeline.get(track_id).unwrap().note_clips[0];
        assert_eq!(section_clip.name, "Section Pattern");
        assert_eq!(section_clip.position_beats, 0.0);
    }

    #[test]
    fn shortening_only_changes_the_playable_boundary() {
        let track_id = TrackId::new();
        let mut section = Section::new(0);
        let hidden_clip = UiNoteClip {
            id: ClipId::new(),
            name: "Fill".into(),
            position_beats: 12.0,
            duration_beats: 4.0,
            notes: Vec::new(),
            selected_notes: Default::default(),
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
        };
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .note_clips
            .push(hidden_clip.clone());

        section.length_beats = 8.0;
        assert!(!section.contains_playable_beat(hidden_clip.position_beats));
        assert_eq!(
            section.timeline.get(track_id).unwrap().note_clips[0].id,
            hidden_clip.id
        );

        section.length_beats = 16.0;
        assert!(section.contains_playable_beat(hidden_clip.position_beats));
        assert_eq!(
            section.timeline.get(track_id).unwrap().note_clips[0].id,
            hidden_clip.id
        );
    }

    #[test]
    fn playback_adapter_prepares_every_project_track_for_intentional_silence() {
        let populated_id = TrackId::new();
        let silent_id = TrackId::new();
        let mut section = Section::new(0);
        Arc::make_mut(&mut section.timeline)
            .ensure(populated_id)
            .note_clips
            .push(UiNoteClip {
                id: ClipId::new(),
                name: "Pattern".into(),
                position_beats: 0.0,
                duration_beats: 4.0,
                notes: Vec::new(),
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
            });
        let tracks = [
            crate::state::ProjectTrack::new(populated_id, "Drums".into(), 0),
            crate::state::ProjectTrack::new(silent_id, "Bass".into(), 1),
        ];

        let prepared = section.prepare_playback_source(&tracks);

        assert_eq!(prepared.section_id, section.id);
        assert_eq!(prepared.tracks().len(), 2);
        assert_eq!(prepared.tracks()[0].track_id, populated_id);
        assert_eq!(prepared.tracks()[0].source.note_clips.len(), 1);
        assert_eq!(prepared.tracks()[1].track_id, silent_id);
        assert!(prepared.tracks()[1].source.clips.is_empty());
        assert!(prepared.tracks()[1].source.note_clips.is_empty());
        assert!(prepared.tracks()[1].source.automation.is_empty());
    }

    #[test]
    fn content_locations_include_arrange_and_every_affected_section() {
        let track_id = TrackId::new();
        let mut arrange = ArrangementTimeline::default();
        arrange
            .ensure(track_id)
            .automation
            .push(vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackGain,
            ));
        let mut first = Section::new(0);
        first.name = "Verse".into();
        Arc::make_mut(&mut first.timeline)
            .ensure(track_id)
            .automation
            .push(vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackPan,
            ));
        let mut second = Section::new(5);
        second.name = "Fill".into();
        Arc::make_mut(&mut second.timeline)
            .ensure(track_id)
            .automation
            .push(vibez_core::automation::AutomationLane::new(
                vibez_core::automation::AutomationTarget::TrackGain,
            ));
        let sections = SectionStore {
            sections: vec![first, second],
        };

        assert_eq!(
            sections.track_content_locations(&arrange, track_id),
            [
                TimelineContentLocation::Arrange,
                TimelineContentLocation::Section {
                    slot: 0,
                    name: "Verse".into()
                },
                TimelineContentLocation::Section {
                    slot: 5,
                    name: "Fill".into()
                }
            ]
        );
    }
}
