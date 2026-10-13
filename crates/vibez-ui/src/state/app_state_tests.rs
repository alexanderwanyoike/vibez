//! Existing shared application state regressions.

use super::*;
use vibez_core::id::TrackId;
use vibez_core::midi::TrackKind;

fn make_state_with(tracks: Vec<ProjectTrack>) -> AppState {
    let mut state = AppState::default();
    Arc::make_mut(&mut state.project_tracks).tracks = tracks;
    state
}

fn make_two_tracks() -> Vec<ProjectTrack> {
    vec![
        ProjectTrack::new(TrackId::new(), "Track 1".into(), 0),
        ProjectTrack::new(TrackId::new(), "Track 2".into(), 1),
    ]
}

#[test]
fn move_track_up() {
    let mut state = make_state_with(make_two_tracks());
    let id0 = state.project_tracks.tracks[0].id;
    let id1 = state.project_tracks.tracks[1].id;

    if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id1) {
        if idx > 0 {
            Arc::make_mut(&mut state.project_tracks)
                .tracks
                .swap(idx, idx - 1);
        }
    }
    assert_eq!(state.project_tracks.tracks[0].id, id1);
    assert_eq!(state.project_tracks.tracks[1].id, id0);
}

#[test]
fn move_track_down() {
    let mut state = make_state_with(make_two_tracks());
    let id0 = state.project_tracks.tracks[0].id;
    let id1 = state.project_tracks.tracks[1].id;

    if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
        if idx + 1 < state.project_tracks.tracks.len() {
            Arc::make_mut(&mut state.project_tracks)
                .tracks
                .swap(idx, idx + 1);
        }
    }
    assert_eq!(state.project_tracks.tracks[0].id, id1);
    assert_eq!(state.project_tracks.tracks[1].id, id0);
}

#[test]
fn move_first_track_up_noop() {
    let mut state = make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
    let id0 = state.project_tracks.tracks[0].id;

    if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
        if idx > 0 {
            Arc::make_mut(&mut state.project_tracks)
                .tracks
                .swap(idx, idx - 1);
        }
    }
    assert_eq!(state.project_tracks.tracks[0].id, id0);
}

#[test]
fn move_last_track_down_noop() {
    let mut state = make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
    let id0 = state.project_tracks.tracks[0].id;

    if let Some(idx) = state.project_tracks.tracks.iter().position(|t| t.id == id0) {
        if idx + 1 < state.project_tracks.tracks.len() {
            Arc::make_mut(&mut state.project_tracks)
                .tracks
                .swap(idx, idx + 1);
        }
    }
    assert_eq!(state.project_tracks.tracks[0].id, id0);
}

#[test]
fn rename_track() {
    let mut state = make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
    let id = state.project_tracks.tracks[0].id;

    if let Some(track) = state.find_track_mut(id) {
        track.name = "My Custom Track".into();
    }
    assert_eq!(state.project_tracks.tracks[0].name, "My Custom Track");
}

#[test]
fn rename_note_clip() {
    let tid = TrackId::new();
    let cid = ClipId::new();
    let track = ProjectTrack::new_instrument(
        tid,
        "Synth".into(),
        TrackKind::Instrument(vibez_core::midi::InstrumentKind::SubtractiveSynth),
        0,
    );
    let mut state = make_state_with(vec![track]);
    state.arrange_content_mut(tid).note_clips.push(UiNoteClip {
        id: cid,
        name: "Pattern 1".into(),
        position_beats: 0.0,
        duration_beats: 4.0,
        notes: Vec::new(),
        selected_notes: HashSet::new(),
        start_marker_beats: 0.0,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    });
    if let Some(t) = Arc::make_mut(&mut state.arrangement.timeline).get_mut(tid) {
        if let Some(c) = t.note_clips.iter_mut().find(|c| c.id == cid) {
            c.name = "Intro Pattern".into();
        }
    }
    assert_eq!(
        state.arrange_content(tid).unwrap().note_clips[0].name,
        "Intro Pattern"
    );
}

#[test]
fn timeline_edits_clone_only_timeline_content() {
    let track_id = TrackId::new();
    let mut state = make_state_with(vec![ProjectTrack::new(
        track_id,
        "Shared Project Track".into(),
        0,
    )]);
    state.arrange_content_mut(track_id);

    let project_tracks_before = Arc::clone(&state.project_tracks);
    let timeline_before = Arc::clone(&state.arrangement.timeline);
    state.arrange_content_mut(track_id).automation.push(
        vibez_core::automation::AutomationLane::new(
            vibez_core::automation::AutomationTarget::TrackGain,
        ),
    );

    assert!(Arc::ptr_eq(&project_tracks_before, &state.project_tracks));
    assert!(!Arc::ptr_eq(&timeline_before, &state.arrangement.timeline));
    assert_eq!(state.project_tracks.tracks[0].name, "Shared Project Track");
}

#[test]
fn settings_tab_default() {
    assert_eq!(SettingsTab::default(), SettingsTab::Audio);
}

#[test]
fn settings_tab_equality() {
    assert_ne!(SettingsTab::Audio, SettingsTab::Plugins);
    assert_eq!(SettingsTab::Audio, SettingsTab::Audio);
}

#[test]
fn app_state_default_buffer_size() {
    let state = AppState::default();
    assert_eq!(state.audio_settings.buffer_size, 512);
}

#[test]
fn app_state_default_settings_tab() {
    let state = AppState::default();
    assert_eq!(state.settings_tab, SettingsTab::Audio);
}

#[test]
fn app_state_opens_in_perform_workspace() {
    let state = AppState::default();
    assert_eq!(state.view.workspace, Workspace::Perform);
}

#[test]
fn transient_analysis_accepts_knob_and_exact_percent_edits() {
    let mut state = AppState::default();
    let mut dialog = TransientAnalysisDialog {
        location: vibez_project::TimelineLocation::Arrange,
        track_id: TrackId::new(),
        clip_id: ClipId::new(),
        sensitivity: vibez_core::onset::TransientSensitivity::DEFAULT,
        sensitivity_input: "50".into(),
    };

    dialog.set_sensitivity(73);
    assert_eq!(dialog.sensitivity.percent(), 73);
    assert_eq!(dialog.sensitivity_input, "73");

    dialog.edit_sensitivity_input("84%".into());
    assert_eq!(dialog.sensitivity.percent(), 84);
    dialog.edit_sensitivity_input("nope".into());
    assert_eq!(dialog.sensitivity.percent(), 84);
    assert!(!dialog.commit_sensitivity_input());
    assert_eq!(dialog.sensitivity_input, "nope");
    dialog.normalize_sensitivity_input();
    assert_eq!(dialog.sensitivity_input, "84");

    state.view.transient_analysis_dialog = Some(dialog);
    assert_eq!(
        state
            .view
            .transient_analysis_dialog
            .unwrap()
            .sensitivity
            .percent(),
        84
    );
}

#[test]
fn deleting_an_audio_clip_dismisses_every_modal_targeting_it() {
    let mut view = ViewState::default();
    let location = vibez_project::TimelineLocation::Arrange;
    let track_id = TrackId::new();
    let clip_id = ClipId::new();
    view.drum_rack_slice_dialog = Some(DrumRackSliceDialog {
        location,
        track_id,
        clip_id,
        markers: crate::domains::arrangement::AudioSliceMarkers::Transients,
    });
    view.transient_analysis_dialog = Some(TransientAnalysisDialog {
        location,
        track_id,
        clip_id,
        sensitivity: vibez_core::onset::TransientSensitivity::DEFAULT,
        sensitivity_input: "50".into(),
    });

    view.dismiss_clip_dialogs_for(location, &HashSet::from([(track_id, clip_id)]));

    assert!(view.drum_rack_slice_dialog.is_none());
    assert!(view.transient_analysis_dialog.is_none());
}

#[test]
fn drum_pad_activity_is_track_and_pitch_specific_and_expires() {
    let mut view = ViewState::default();
    let track_id = TrackId::new();
    let other_track_id = TrackId::new();
    let now = std::time::Instant::now();

    view.trigger_drum_pad_flash(track_id, 52, now);

    assert!(view.drum_pad_is_flashing(track_id, 52, now));
    assert!(!view.drum_pad_is_flashing(track_id, 51, now));
    assert!(!view.drum_pad_is_flashing(other_track_id, 52, now));

    let after_hold = now + ViewState::DRUM_PAD_FLASH_HOLD;
    assert!(!view.drum_pad_is_flashing(track_id, 52, after_hold));
    view.prune_drum_pad_flashes(after_hold);
    assert!(view.drum_pad_flash_until.is_empty());
}

#[test]
fn drum_pad_bank_activity_finds_flashes_outside_the_selected_bank() {
    let mut view = ViewState::default();
    let track_id = TrackId::new();
    let now = std::time::Instant::now();
    let second_bank_pitch =
        vibez_core::track::drum_rack_pad_pitch(vibez_core::track::DRUM_RACK_BANK_SIZE).unwrap();

    view.trigger_drum_pad_flash(track_id, second_bank_pitch, now);

    assert!(!view.drum_pad_bank_is_flashing(track_id, 0, now));
    assert!(view.drum_pad_bank_is_flashing(track_id, 1, now));
}

#[test]
fn rename_empty_rejected() {
    let mut state = make_state_with(vec![ProjectTrack::new(TrackId::new(), "Track 1".into(), 0)]);
    let id = state.project_tracks.tracks[0].id;

    // Simulate the FinishEditing guard: empty name doesn't rename
    let new_name = "";
    if !new_name.is_empty() {
        if let Some(track) = state.find_track_mut(id) {
            track.name = new_name.to_string();
        }
    }
    assert_eq!(state.project_tracks.tracks[0].name, "Track 1");
}
