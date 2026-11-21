//! How a MIDI drop lands, checked against the project as it stands.

use std::sync::Arc;

use super::*;
use crate::domains::perform::{LauncherClip, Section, LAUNCHER_SLOT_GONE};
use crate::state::ProjectTrack;
use vibez_core::id::ClipId;
use vibez_core::midi::TrackKind;
use vibez_project::PerformLayout;

struct Project {
    tracks: ProjectTracksState,
    perform: PerformState,
    midi: TrackId,
    audio: TrackId,
}

impl Project {
    fn new() -> Self {
        let midi = TrackId::new();
        let audio = TrackId::new();
        let mut tracks = ProjectTracksState::default();
        tracks.tracks.push(ProjectTrack::new_instrument(
            midi,
            "Drums".into(),
            TrackKind::Midi,
            0,
        ));
        tracks
            .tracks
            .push(ProjectTrack::new(audio, "Audio 1".into(), 1));
        Self {
            tracks,
            perform: PerformState::default(),
            midi,
            audio,
        }
    }

    fn resolve(&self, target: BrowserImportTarget) -> Result<MidiLanding, String> {
        MidiLanding::resolve(
            &target,
            &MidiLandingCtx {
                project_tracks: &self.tracks,
                perform: &self.perform,
                samples_per_beat: 1_000.0,
                playhead_beats: 6.0,
            },
        )
    }
}

#[test]
fn a_midi_lane_takes_the_drop_at_its_beat_and_an_audio_lane_refuses_it() {
    let project = Project::new();

    assert_eq!(
        project.resolve(BrowserImportTarget::ArrangementClipAt {
            track_id: project.midi,
            position_samples: 8_000,
        }),
        Ok(MidiLanding::Arrange {
            onto: Some(project.midi),
            beat: 8.0,
        })
    );
    assert_eq!(
        project.resolve(BrowserImportTarget::ArrangementClipAt {
            track_id: project.audio,
            position_samples: 0,
        }),
        Err(MIDI_FILE_TARGETS.into())
    );
}

#[test]
fn an_import_at_the_playhead_keeps_only_a_selected_midi_track() {
    let project = Project::new();

    assert_eq!(
        project.resolve(BrowserImportTarget::ArrangementClip(Some(project.audio))),
        Ok(MidiLanding::Arrange {
            onto: None,
            beat: 6.0,
        })
    );
    assert_eq!(
        project
            .resolve(BrowserImportTarget::ArrangementClip(Some(project.midi)))
            .map(MidiLanding::onto),
        Ok(Some(project.midi))
    );
}

#[test]
fn a_section_target_must_still_exist() {
    let mut project = Project::new();
    let section = Section::new(0);
    let section_id = section.id;
    Arc::make_mut(&mut project.perform.sections).insert(section);
    let target = |section_id| BrowserImportTarget::SectionClipAt {
        section_id,
        track_id: project.midi,
        position_samples: 2_000,
    };

    assert_eq!(
        project.resolve(target(section_id)),
        Ok(MidiLanding::Section {
            section_id,
            onto: Some(project.midi),
            beat: 2.0,
        })
    );
    assert_eq!(
        project.resolve(target(SectionId::new())),
        Err(MIDI_FILE_TARGETS.into())
    );
}

#[test]
fn a_clip_slot_must_be_free_in_a_clips_project() {
    let mut project = Project::new();
    let slot = BrowserImportTarget::LauncherClipAt {
        track_id: project.midi,
        row: 1,
    };
    assert_eq!(
        project.resolve(slot.clone()),
        Err(LAUNCHER_SLOT_GONE.into())
    );

    project.perform.layout = PerformLayout::Clips;
    assert_eq!(
        project.resolve(slot.clone()),
        Ok(MidiLanding::Slot {
            onto: Some(project.midi),
            row: 1,
        })
    );

    Arc::make_mut(&mut project.perform.clips)
        .clips
        .push(LauncherClip {
            id: ClipId::new(),
            track_id: project.midi,
            row: 1,
            timeline: Default::default(),
            launch: Default::default(),
        });
    assert_eq!(project.resolve(slot), Err(LAUNCHER_SLOT_GONE.into()));
}

#[test]
fn instrument_and_device_targets_refuse_a_midi_file() {
    let project = Project::new();
    for target in [
        BrowserImportTarget::Sampler(project.midi),
        BrowserImportTarget::ImpliedSampler(project.midi),
        BrowserImportTarget::DrumRackPad {
            track_id: project.midi,
            pad_index: 0,
        },
    ] {
        assert_eq!(project.resolve(target), Err(MIDI_FILE_TARGETS.into()));
    }
}

#[test]
fn the_status_names_the_track_the_slot_or_the_count_of_new_tracks() {
    let lane = MidiLanding::Arrange {
        onto: None,
        beat: 8.0,
    };
    let slot = MidiLanding::Slot {
        onto: Some(TrackId::new()),
        row: 1,
    };

    assert_eq!(
        lane.status("Rebeater P3", &["Drums"]),
        "Imported 'Rebeater P3' to Drums at beat 8.00"
    );
    assert_eq!(
        slot.status("Rebeater P3", &["Drums"]),
        "Added Rebeater P3 to Clip row 2"
    );
    assert_eq!(
        slot.status("Rebeater P3", &["Kick", "Snare"]),
        "Imported 'Rebeater P3' as 2 new MIDI tracks"
    );
}
