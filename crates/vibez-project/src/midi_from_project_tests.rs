//! A track's MIDI From survives a save and reopen in both project formats, and old projects load without one.

use super::*;
use vibez_core::{id::TrackId, midi_from::MidiFrom};

#[test]
fn midi_from_survives_legacy_json_and_the_project_container() {
    let directory = std::env::temp_dir().join(format!(
        "vibez-midi-from-project-test-{}-{}",
        std::process::id(),
        TrackId::new().raw()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let mut project = Project::default();
    let source = TrackInfo::new("Rebeater");
    let mut receiver = TrackInfo::new("Lead");
    receiver.midi_from = MidiFrom::new(receiver.id, source.id);
    project.tracks.extend([source.clone(), receiver.clone()]);

    let json_path = directory.join("routed.vibez");
    project.save_to_file(&json_path).unwrap();
    let reopened = Project::load_from_file(&json_path).unwrap();
    assert_eq!(reopened.tracks[1].midi_from, receiver.midi_from);
    assert_eq!(reopened.tracks[0].midi_from, None);

    let container = directory.join("routed.vzp");
    project_format_v1::save_project_v1(&container, None, project).unwrap();
    let reopened = project_format_v1::ProjectContainer::open(&container)
        .unwrap()
        .load_document()
        .unwrap();
    assert_eq!(reopened.project.tracks[1].midi_from, receiver.midi_from);
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn a_project_saved_before_midi_from_loads_with_no_routing() {
    let legacy = format!(
        r#"{{"name":"Legacy","bpm":120,"sample_rate":48000,"tracks":[{}],"clips":[],"note_clips":[]}}"#,
        serde_json::json!({"id": 7, "name": "Lead", "gain": 1.0, "pan": 0.5, "mute": false, "solo": false})
    );
    let project: Project = serde_json::from_str(&legacy).unwrap();
    assert_eq!(project.tracks[0].midi_from, None);
}
