//! Planning the Clips and tracks a dropped MIDI file lands as.

use super::*;
use vibez_core::midi::MidiNote;
use vibez_core::midi_file::MidiFileTrack;

fn hit(pitch: u8, start_beat: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity: 100,
        start_beat,
        duration_beats: 0.25,
    }
}

fn track(name: Option<&str>, notes: Vec<MidiNote>, on_drum_channel: bool) -> MidiFileTrack {
    MidiFileTrack {
        name: name.map(Into::into),
        notes,
        on_drum_channel,
    }
}

fn import(tracks: Vec<MidiFileTrack>) -> MidiFileImport {
    MidiFileImport {
        name: "Rebeater P3".into(),
        file: MidiFile {
            tracks,
            length_beats: 16.0,
        },
    }
}

fn drum_phrase() -> MidiFileImport {
    import(vec![track(
        None,
        vec![hit(36, 0.0), hit(42, 0.5), hit(38, 1.0)],
        true,
    )])
}

fn split_phrase() -> MidiFileImport {
    import(vec![
        track(Some("Kick"), vec![hit(36, 0.0), hit(36, 2.0)], true),
        track(Some("Closed Hat"), vec![hit(42, 0.5)], true),
        track(None, vec![hit(60, 1.0)], false),
    ])
}

#[test]
fn a_file_on_a_track_is_one_clip_there_at_the_drop_spanning_the_file() {
    let onto = TrackId::new();
    let file = drum_phrase();

    let plan = file.plan(Some(onto), 8.0, MidiFileLayout::OneClip);

    assert_eq!(plan.len(), 1);
    assert_eq!(plan[0].track, MidiClipTrack::Existing(onto));
    let clip = &plan[0].clip;
    assert_eq!(clip.name, "Rebeater P3");
    assert_eq!(clip.position_beats, 8.0);
    assert_eq!(clip.duration_beats, 16.0);
    assert_eq!(clip.notes, file.file.tracks[0].notes);
    assert_eq!(clip.start_marker_beats, 0.0);
    assert!(clip.loop_enabled);
    assert_eq!((clip.loop_start_beats, clip.loop_end_beats), (0.0, 16.0));
    assert!(clip.selected_notes.is_empty());
}

#[test]
fn drums_without_a_track_land_on_a_new_drum_rack_track_named_for_the_file() {
    let plan = drum_phrase().plan(None, 0.0, MidiFileLayout::OneClip);

    assert_eq!(
        plan[0].track,
        MidiClipTrack::New {
            name: "Rebeater P3".into(),
            drum_rack: true,
        }
    );
}

#[test]
fn melodic_notes_without_a_track_land_on_a_new_track_without_a_drum_rack() {
    let file = import(vec![track(None, vec![hit(60, 0.0)], false)]);

    let plan = file.plan(None, 0.0, MidiFileLayout::OneClip);

    assert_eq!(
        plan[0].track,
        MidiClipTrack::New {
            name: "Rebeater P3".into(),
            drum_rack: false,
        }
    );
}

#[test]
fn only_a_file_with_several_playing_tracks_offers_layouts() {
    assert!(!drum_phrase().offers_layouts());
    assert!(split_phrase().offers_layouts());
}

#[test]
fn one_clip_merges_every_file_track_in_time() {
    let onto = TrackId::new();

    let plan = split_phrase().plan(Some(onto), 0.0, MidiFileLayout::OneClip);

    assert_eq!(plan.len(), 1);
    let starts: Vec<_> = plan[0]
        .clip
        .notes
        .iter()
        .map(|note| (note.start_beat, note.pitch))
        .collect();
    assert_eq!(starts, vec![(0.0, 36), (0.5, 42), (1.0, 60), (2.0, 36)]);
}

#[test]
fn a_track_per_file_track_names_each_new_track_and_clip_after_its_file_track() {
    let plan = split_phrase().plan(Some(TrackId::new()), 4.0, MidiFileLayout::TrackPerFileTrack);

    let tracks: Vec<_> = plan.iter().map(|planned| planned.track.clone()).collect();
    assert_eq!(
        tracks,
        vec![
            MidiClipTrack::New {
                name: "Kick".into(),
                drum_rack: true,
            },
            MidiClipTrack::New {
                name: "Closed Hat".into(),
                drum_rack: true,
            },
            MidiClipTrack::New {
                name: "Rebeater P3 3".into(),
                drum_rack: false,
            },
        ]
    );
    let names: Vec<_> = plan
        .iter()
        .map(|planned| planned.clip.name.as_str())
        .collect();
    assert_eq!(names, vec!["Kick", "Closed Hat", "Rebeater P3 3"]);
    assert_eq!(plan[1].clip.notes, vec![hit(42, 0.5)]);
    assert!(plan
        .iter()
        .all(|planned| planned.clip.position_beats == 4.0 && planned.clip.duration_beats == 16.0));
}

#[test]
fn every_planned_clip_has_its_own_identity() {
    let plan = split_phrase().plan(None, 0.0, MidiFileLayout::TrackPerFileTrack);

    let ids: HashSet<_> = plan.iter().map(|planned| planned.clip.id).collect();
    assert_eq!(ids.len(), plan.len());
}
