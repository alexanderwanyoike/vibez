//! Canonical project/runtime projection for Arrange and Section timelines.

use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::track::{ClipInfo, MediaSourceRef};
use vibez_project::{SectionInfo, TimelineAutomationInfo, TimelineInfo, TimelineLocation};

use crate::domains::perform::{Section, SectionStore};
use crate::state::{AppState, ArrangementTimeline, UiClip, UiNoteClip};

pub(super) fn timeline_info_from_ui(timeline: &ArrangementTimeline) -> TimelineInfo {
    let mut track_ids: Vec<_> = timeline.by_track.keys().copied().collect();
    track_ids.sort_by_key(|id| id.raw());
    let mut info = TimelineInfo::default();
    for track_id in track_ids {
        let content = &timeline.by_track[&track_id];
        info.clips.extend(content.clips.iter().map(|clip| ClipInfo {
            id: clip.id,
            track_id,
            name: clip.name.clone(),
            position: clip.position,
            source_offset: clip.source_offset,
            duration: clip.duration,
            source: clip.source.clone(),
            file_path: clip.source.as_ref().and_then(|source| match source {
                MediaSourceRef::LocalFile { path } => Some(path.clone()),
                _ => None,
            }),
            loop_enabled: clip.loop_enabled,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
            original_bpm: clip.original_bpm,
            warped: clip.warped,
            warped_to_bpm: clip.warped_to_bpm,
        }));
        info.note_clips
            .extend(
                content
                    .note_clips
                    .iter()
                    .map(|clip| vibez_core::midi::NoteClipInfo {
                        id: clip.id,
                        track_id,
                        name: clip.name.clone(),
                        position_beats: clip.position_beats,
                        duration_beats: clip.duration_beats,
                        notes: clip.notes.clone(),
                        loop_enabled: clip.loop_enabled,
                        loop_start_beats: clip.loop_start_beats,
                        loop_end_beats: clip.loop_end_beats,
                    }),
            );
        if !content.automation.is_empty() {
            info.automation.push(TimelineAutomationInfo {
                track_id,
                lanes: content.automation.clone(),
            });
        }
    }
    info
}

pub(super) fn section_info_from_ui(section: &Section) -> SectionInfo {
    SectionInfo {
        id: section.id,
        slot: section.slot,
        name: section.name.clone(),
        length_beats: section.length_beats,
        launch_quantization: section.launch_quantization,
        looping: section.looping,
        timeline: timeline_info_from_ui(&section.timeline),
    }
}

pub(super) fn section_store_from_project(sections: &[SectionInfo]) -> SectionStore {
    let mut store = SectionStore::default();
    for info in sections {
        store.insert(Section {
            id: info.id,
            slot: info.slot,
            name: info.name.clone(),
            length_beats: info.length_beats,
            launch_quantization: info.launch_quantization,
            looping: info.looping,
            timeline: Arc::new(timeline_without_audio(&info.timeline)),
        });
    }
    store
}

pub(super) fn timeline_without_audio(info: &TimelineInfo) -> ArrangementTimeline {
    let mut timeline = ArrangementTimeline::default();
    for automation in &info.automation {
        timeline.ensure(automation.track_id).automation = automation.lanes.clone();
    }
    for clip in &info.note_clips {
        timeline.ensure(clip.track_id).note_clips.push(UiNoteClip {
            id: clip.id,
            name: clip.name.clone(),
            position_beats: clip.position_beats,
            duration_beats: clip.duration_beats,
            notes: clip.notes.clone(),
            selected_notes: HashSet::new(),
            loop_enabled: clip.loop_enabled,
            loop_start_beats: clip.loop_start_beats,
            loop_end_beats: clip.loop_end_beats,
        });
    }
    timeline
}

pub(super) fn runtime_timeline_mut(
    state: &mut AppState,
    location: TimelineLocation,
) -> Option<&mut Arc<ArrangementTimeline>> {
    match location {
        TimelineLocation::Arrange => Some(&mut state.arrangement.timeline),
        TimelineLocation::Section(id) => Arc::make_mut(&mut state.perform.sections)
            .by_id_mut(id)
            .map(|section| &mut section.timeline),
    }
}

pub(super) fn install_loaded_clip(
    timeline: &mut ArrangementTimeline,
    loaded: crate::message::LoadedClipData,
) {
    timeline.ensure(loaded.info.track_id).clips.push(UiClip {
        id: loaded.info.id,
        name: loaded.info.name,
        audio: loaded.audio,
        source: loaded.info.source.clone(),
        position: loaded.info.position,
        source_offset: loaded.info.source_offset,
        duration: loaded.info.duration,
        loop_enabled: loaded.info.loop_enabled,
        loop_start: loaded.info.loop_start,
        loop_end: loaded.info.loop_end,
        original_bpm: loaded.info.original_bpm,
        warped: loaded.info.warped,
        warped_to_bpm: loaded.info.warped_to_bpm,
        original_audio: loaded.original_audio,
    });
}

pub(super) fn apply_timeline_sources(timeline: &mut ArrangementTimeline, saved: &TimelineInfo) {
    for saved_clip in &saved.clips {
        if let Some(clip) = timeline.get_mut(saved_clip.track_id).and_then(|content| {
            content
                .clips
                .iter_mut()
                .find(|clip| clip.id == saved_clip.id)
        }) {
            clip.source = saved_clip.source.clone();
        }
    }
}

pub(super) fn legacy_automation_for_track(
    project: &vibez_project::Project,
    track_id: vibez_core::id::TrackId,
) -> Vec<vibez_core::automation::AutomationLane> {
    project
        .arrange
        .automation
        .iter()
        .find(|automation| automation.track_id == track_id)
        .map(|automation| automation.lanes.clone())
        .or_else(|| {
            project
                .tracks
                .iter()
                .chain(project.master.iter())
                .chain(project.buses.iter())
                .find(|track| track.id == track_id)
                .map(|track| track.automation.clone())
        })
        .unwrap_or_default()
}
