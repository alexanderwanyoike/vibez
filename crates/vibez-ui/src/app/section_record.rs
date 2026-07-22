//! Application boundary for Section Record residency and project transaction.

use iced::Task;
use std::sync::Arc;
use vibez_core::id::ClipId;
use vibez_core::midi::MidiNote;
use vibez_core::perform::GrooveGrid;
use vibez_engine::commands::EngineCommand;

use crate::domains::perform::{
    CompletedSectionRecording, SectionRecordAction, SectionRecordStartRequest,
};
use crate::message::{Message, ResidentSection};
use crate::state::UiNoteClip;

use super::*;

const RECORD_CLIP_OFF: &str = "Section Record";
const RECORD_CLIP_EIGHTH: &str = "Section Record · 1/8 Groove";
const RECORD_CLIP_SIXTEENTH: &str = "Section Record · 1/16 Groove";

impl App {
    pub(super) fn apply_section_record_action(
        &mut self,
        action: SectionRecordAction,
    ) -> Task<Message> {
        match action {
            SectionRecordAction::Start(request) => self.begin_section_record(request),
            SectionRecordAction::Stop => {
                if self.state.perform.section_record.arm_was_sent() {
                    self.send_command(EngineCommand::StopSectionRecord);
                    self.state.status_text = "Stopping Section Record…".into();
                } else {
                    self.section_residency_request.cancel();
                    self.state.perform.section_record.cancel();
                    self.discard_section_record_transaction();
                    self.state.status_text = "Section Record cancelled".into();
                }
                Task::none()
            }
        }
    }

    fn begin_section_record(&mut self, request: SectionRecordStartRequest) -> Task<Message> {
        if !self.begin_project_transaction() {
            self.state.perform.section_record.cancel();
            self.state.status_text = "Finish the current project edit before recording".into();
            return Task::none();
        }
        if !request.from_stopped {
            self.send_section_record_arm(request, None);
            return Task::none();
        }

        let Some(section) = self
            .state
            .perform
            .sections
            .by_id(request.section_id)
            .cloned()
        else {
            self.state.perform.section_record.cancel();
            self.discard_section_record_transaction();
            return Task::none();
        };
        let track_ids: Vec<_> = self
            .state
            .project_tracks
            .tracks
            .iter()
            .map(|track| track.id)
            .collect();
        let request_id = self.section_residency_request.begin();
        let task = Task::perform(
            async move {
                let prepared = tokio::task::spawn_blocking(move || {
                    section.prepare_playback_source_for_tracks(&track_ids)
                })
                .await
                .expect("Section Record residency worker panicked");
                ResidentSection::new(Box::new(prepared))
            },
            move |resident| Message::SectionRecordResidencyReady {
                request_id,
                request,
                resident,
            },
        );
        self.state.status_text = "Preparing Section Record…".into();
        self.section_residency_request.attach(task)
    }

    pub(super) fn finish_section_record_residency(
        &mut self,
        request_id: u64,
        request: SectionRecordStartRequest,
        resident: ResidentSection,
    ) {
        if !self.section_residency_request.finish(request_id)
            || self.state.perform.section_record.target()
                != Some((request.section_id, request.track_id))
        {
            return;
        }
        if let Some(prepared) = resident.take() {
            self.send_section_record_arm(request, Some(prepared));
        }
    }

    fn send_section_record_arm(
        &mut self,
        request: SectionRecordStartRequest,
        prepared: Option<Box<vibez_engine::playback_source::PreparedSectionPlaybackSource>>,
    ) {
        self.state.perform.section_record.mark_arm_sent();
        self.send_command(EngineCommand::ArmSectionRecord {
            section_id: request.section_id,
            track_id: request.track_id,
            prepared,
            count_in_bars: request.count_in_bars,
        });
        self.state.status_text = "Section Record armed".into();
    }

    pub(super) fn finish_section_record_session(
        &mut self,
        completed: Option<CompletedSectionRecording>,
    ) {
        let changed = completed
            .map(|recording| self.apply_completed_section_recording(recording))
            .unwrap_or(false);
        if changed {
            self.push_undo_snapshot(None);
            self.mark_project_dirty();
            self.commit_project_transaction();
            self.state.status_text = "Section Record committed · one undo step".into();
        } else {
            self.discard_section_record_transaction();
            self.state.status_text = "Section Record stopped · no notes changed".into();
        }
    }

    fn discard_section_record_transaction(&mut self) {
        if let Some((_, dirty_before)) = self.state.project.history.abandon_transaction() {
            self.state.project.dirty = dirty_before;
        }
    }

    fn apply_completed_section_recording(&mut self, recording: CompletedSectionRecording) -> bool {
        let Some(section) =
            Arc::make_mut(&mut self.state.perform.sections).by_id_mut(recording.section_id)
        else {
            return false;
        };
        let changed = apply_recording_to_section(section, &recording);
        if changed {
            self.state
                .perform
                .sync_selected_section_editor(self.state.arrangement.selected_track);
            self.refresh_playing_section_after_edit(recording.section_id);
        }
        changed
    }
}

fn apply_recording_to_section(
    section: &mut crate::domains::perform::Section,
    recording: &CompletedSectionRecording,
) -> bool {
    let section_length = section.length_beats;
    let content = Arc::make_mut(&mut section.timeline).ensure(recording.track_id);
    let mut changed = false;
    if !recording.replace_ranges.is_empty() {
        for clip in &mut content.note_clips {
            let before = clip.notes.len();
            clip.notes.retain(|note| {
                let start = clip.position_beats + note.start_beat;
                !recording
                    .replace_ranges
                    .iter()
                    .any(|(from, to)| start >= *from && start < *to)
            });
            changed |= clip.notes.len() != before;
        }
    }

    for grid in [GrooveGrid::Off, GrooveGrid::Eighth, GrooveGrid::Sixteenth] {
        let incoming: Vec<_> = recording
            .notes
            .iter()
            .filter(|note| note.groove_grid == grid)
            .map(|note| MidiNote {
                pitch: note.pitch,
                velocity: note.velocity,
                start_beat: note.start_beat,
                duration_beats: note.duration_beats,
            })
            .collect();
        if incoming.is_empty() {
            continue;
        }
        let name = record_clip_name(grid);
        let clip = if let Some(index) = content.note_clips.iter().position(|clip| {
            clip.name == name && clip.position_beats == 0.0 && clip.groove_grid == grid
        }) {
            &mut content.note_clips[index]
        } else {
            content.note_clips.push(UiNoteClip {
                id: ClipId::new(),
                name: name.into(),
                position_beats: 0.0,
                duration_beats: section_length,
                notes: Vec::new(),
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
                groove_grid: grid,
            });
            content.note_clips.last_mut().expect("record clip")
        };
        clip.duration_beats = section_length;
        clip.notes.extend(incoming);
        clip.notes.sort_by(|left, right| {
            left.start_beat
                .total_cmp(&right.start_beat)
                .then(left.pitch.cmp(&right.pitch))
        });
        changed = true;
    }

    changed
}

fn record_clip_name(grid: GrooveGrid) -> &'static str {
    match grid {
        GrooveGrid::Off => RECORD_CLIP_OFF,
        GrooveGrid::Eighth => RECORD_CLIP_EIGHTH,
        GrooveGrid::Sixteenth => RECORD_CLIP_SIXTEENTH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domains::perform::section_record::RecordedSectionNote;
    use crate::domains::perform::Section;
    use crate::state::TrackTimelineContent;
    use vibez_core::id::TrackId;

    #[test]
    fn replace_clears_only_crossed_note_starts_and_groups_new_grooves() {
        let track_id = TrackId::new();
        let mut section = Section::new(0);
        Arc::make_mut(&mut section.timeline).by_track.insert(
            track_id,
            TrackTimelineContent {
                note_clips: vec![UiNoteClip {
                    id: ClipId::new(),
                    name: "Authored".into(),
                    position_beats: 0.0,
                    duration_beats: 16.0,
                    notes: vec![
                        MidiNote {
                            pitch: 36,
                            velocity: 100,
                            start_beat: 1.0,
                            duration_beats: 0.25,
                        },
                        MidiNote {
                            pitch: 38,
                            velocity: 100,
                            start_beat: 5.0,
                            duration_beats: 0.25,
                        },
                    ],
                    selected_notes: Default::default(),
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                    groove_grid: GrooveGrid::Off,
                }],
                ..TrackTimelineContent::default()
            },
        );
        let recording = CompletedSectionRecording {
            section_id: section.id,
            track_id,
            notes: vec![
                RecordedSectionNote {
                    pitch: 42,
                    velocity: 110,
                    start_beat: 2.0,
                    duration_beats: 0.25,
                    groove_grid: GrooveGrid::Off,
                },
                RecordedSectionNote {
                    pitch: 46,
                    velocity: 105,
                    start_beat: 2.25,
                    duration_beats: 0.25,
                    groove_grid: GrooveGrid::Sixteenth,
                },
            ],
            replace_ranges: vec![(0.5, 4.0)],
        };

        assert!(apply_recording_to_section(&mut section, &recording));
        let clips = &section.timeline.get(track_id).unwrap().note_clips;
        assert_eq!(
            clips[0]
                .notes
                .iter()
                .map(|note| note.pitch)
                .collect::<Vec<_>>(),
            vec![38]
        );
        assert_eq!(clips.len(), 3);
        assert_eq!(clips[1].groove_grid, GrooveGrid::Off);
        assert_eq!(clips[2].groove_grid, GrooveGrid::Sixteenth);
    }
}
