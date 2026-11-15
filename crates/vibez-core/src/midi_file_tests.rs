//! Reading hand-encoded Standard MIDI Files into beat-timed notes.

use super::*;

/// One track event: delta ticks, then its raw bytes after the delta.
type Event = (u32, Vec<u8>);

fn vlq(out: &mut Vec<u8>, value: u32) {
    let mut bytes = vec![(value & 0x7f) as u8];
    let mut rest = value >> 7;
    while rest > 0 {
        bytes.push((rest & 0x7f) as u8 | 0x80);
        rest >>= 7;
    }
    out.extend(bytes.iter().rev());
}

fn track_chunk(events: &[Event]) -> Vec<u8> {
    let mut body = Vec::new();
    for (delta, bytes) in events {
        vlq(&mut body, *delta);
        body.extend_from_slice(bytes);
    }
    let mut chunk = b"MTrk".to_vec();
    chunk.extend_from_slice(&(body.len() as u32).to_be_bytes());
    chunk.extend(body);
    chunk
}

fn smf(format: u16, division: u16, tracks: &[Vec<Event>]) -> Vec<u8> {
    let mut file = b"MThd".to_vec();
    file.extend_from_slice(&6u32.to_be_bytes());
    file.extend_from_slice(&format.to_be_bytes());
    file.extend_from_slice(&(tracks.len() as u16).to_be_bytes());
    file.extend_from_slice(&division.to_be_bytes());
    for track in tracks {
        file.extend(track_chunk(track));
    }
    file
}

fn on(delta: u32, channel: u8, key: u8, velocity: u8) -> Event {
    (delta, vec![0x90 | channel, key, velocity])
}

fn off(delta: u32, channel: u8, key: u8) -> Event {
    (delta, vec![0x80 | channel, key, 0])
}

fn name(text: &str) -> Event {
    let mut bytes = vec![0xff, 0x03, text.len() as u8];
    bytes.extend_from_slice(text.as_bytes());
    (0, bytes)
}

fn end(delta: u32) -> Event {
    (delta, vec![0xff, 0x2f, 0x00])
}

fn note(pitch: u8, velocity: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity,
        start_beat,
        duration_beats,
    }
}

#[test]
fn ticks_become_beats_at_the_files_own_resolution() {
    for ppq in [96u16, 960] {
        let q = u32::from(ppq);
        let bytes = smf(
            0,
            ppq,
            &[vec![
                on(0, 9, 36, 127),
                off(q / 4, 9, 36),
                on(q / 4, 9, 38, 90),
                off(q / 2, 9, 38),
                end(3 * q),
            ]],
        );

        let file = MidiFile::read(&bytes).expect("type 0 file");

        assert_eq!(file.tracks.len(), 1, "at {ppq} PPQ");
        assert_eq!(
            file.tracks[0].notes,
            vec![note(36, 127, 0.0, 0.25), note(38, 90, 0.5, 0.5)],
            "at {ppq} PPQ"
        );
        assert_eq!(file.length_beats, 4.0, "the End of Track at {ppq} PPQ");
    }
}

#[test]
fn a_type_1_file_keeps_each_playing_track_and_its_name() {
    let bytes = smf(
        1,
        480,
        &[
            // A conductor track carries tempo and no notes.
            vec![
                name("Rebeater P3"),
                (0, vec![0xff, 0x51, 0x03, 0x07, 0xa1, 0x20]),
                end(0),
            ],
            vec![name("Kick"), on(0, 9, 36, 127), off(120, 9, 36), end(1800)],
            vec![
                name("Closed Hat"),
                on(240, 9, 42, 95),
                off(120, 9, 42),
                end(1560),
            ],
        ],
    );

    let file = MidiFile::read(&bytes).expect("type 1 file");

    let names: Vec<_> = file.tracks.iter().map(|t| t.name.as_deref()).collect();
    assert_eq!(names, vec![Some("Kick"), Some("Closed Hat")]);
    assert_eq!(file.tracks[1].notes, vec![note(42, 95, 0.5, 0.25)]);
    assert_eq!(file.length_beats, 4.0);
}

#[test]
fn the_longest_track_sets_the_length() {
    let bytes = smf(
        1,
        96,
        &[
            vec![on(0, 0, 60, 100), off(96, 0, 60), end(0)],
            vec![on(0, 0, 64, 100), off(96, 0, 64), end(7 * 96)],
        ],
    );

    assert_eq!(MidiFile::read(&bytes).unwrap().length_beats, 8.0);
}

#[test]
fn a_file_never_ends_before_its_last_note() {
    let bytes = smf(0, 96, &[vec![on(0, 0, 60, 100), off(192, 0, 60), end(0)]]);

    assert_eq!(MidiFile::read(&bytes).unwrap().length_beats, 2.0);
}

#[test]
fn a_zero_velocity_note_on_ends_the_note() {
    let bytes = smf(0, 96, &[vec![on(0, 0, 60, 100), on(48, 0, 60, 0), end(48)]]);

    assert_eq!(
        MidiFile::read(&bytes).unwrap().tracks[0].notes,
        vec![note(60, 100, 0.0, 0.5)]
    );
}

#[test]
fn running_status_reads_like_full_messages() {
    // Status 0x90 once, then key/velocity pairs for every later event.
    let bytes = smf(
        0,
        96,
        &[vec![
            (0, vec![0x90, 36, 100]),
            (48, vec![36, 0]),
            (48, vec![38, 80]),
            (48, vec![38, 0]),
            end(0),
        ]],
    );

    assert_eq!(
        MidiFile::read(&bytes).unwrap().tracks[0].notes,
        vec![note(36, 100, 0.0, 0.5), note(38, 80, 1.0, 0.5)]
    );
}

#[test]
fn overlapping_notes_on_one_key_end_first_in_first_out() {
    let bytes = smf(
        0,
        96,
        &[vec![
            on(0, 0, 60, 100),
            on(96, 0, 60, 50),
            off(96, 0, 60),
            off(96, 0, 60),
            end(0),
        ]],
    );

    assert_eq!(
        MidiFile::read(&bytes).unwrap().tracks[0].notes,
        vec![note(60, 100, 0.0, 2.0), note(60, 50, 1.0, 2.0)]
    );
}

#[test]
fn a_note_left_on_ends_with_its_track() {
    let bytes = smf(0, 96, &[vec![on(96, 0, 60, 100), end(3 * 96)]]);

    assert_eq!(
        MidiFile::read(&bytes).unwrap().tracks[0].notes,
        vec![note(60, 100, 1.0, 3.0)]
    );
}

#[test]
fn a_note_ended_on_its_own_tick_lasts_one_tick() {
    let bytes = smf(0, 96, &[vec![on(0, 9, 36, 100), off(0, 9, 36), end(96)]]);

    assert_eq!(
        MidiFile::read(&bytes).unwrap().tracks[0].notes,
        vec![note(36, 100, 0.0, 1.0 / 96.0)]
    );
}

#[test]
fn notes_on_one_track_sort_by_start_then_pitch() {
    let bytes = smf(
        0,
        96,
        &[vec![
            on(0, 9, 42, 90),
            on(0, 9, 36, 127),
            off(24, 9, 42),
            off(0, 9, 36),
            end(72),
        ]],
    );

    let pitches: Vec<_> = MidiFile::read(&bytes).unwrap().tracks[0]
        .notes
        .iter()
        .map(|n| n.pitch)
        .collect();
    assert_eq!(pitches, vec![36, 42]);
}

#[test]
fn drum_channel_notes_mark_the_track_and_the_file() {
    let drums = smf(0, 96, &[vec![on(0, 9, 36, 100), off(24, 9, 36), end(0)]]);
    let mixed = smf(
        1,
        96,
        &[
            vec![on(0, 9, 36, 100), off(24, 9, 36), end(0)],
            vec![on(0, 0, 60, 100), off(24, 0, 60), end(0)],
        ],
    );

    let drums = MidiFile::read(&drums).unwrap();
    let mixed = MidiFile::read(&mixed).unwrap();

    assert!(drums.tracks[0].on_drum_channel && drums.on_drum_channel());
    assert!(mixed.tracks[0].on_drum_channel);
    assert!(!mixed.tracks[1].on_drum_channel);
    assert!(!mixed.on_drum_channel());
}

#[test]
fn merged_notes_interleave_every_track_in_time() {
    let bytes = smf(
        1,
        96,
        &[
            vec![
                on(0, 9, 36, 127),
                off(24, 9, 36),
                on(72, 9, 36, 100),
                off(24, 9, 36),
                end(0),
            ],
            vec![on(48, 9, 42, 90), off(24, 9, 42), end(0)],
        ],
    );

    let starts: Vec<_> = MidiFile::read(&bytes)
        .unwrap()
        .merged_notes()
        .iter()
        .map(|n| (n.start_beat, n.pitch))
        .collect();
    assert_eq!(starts, vec![(0.0, 36), (0.5, 42), (1.0, 36)]);
}

#[test]
fn smpte_timing_is_refused() {
    // A negative high byte selects frames per second and ticks per frame.
    let bytes = smf(
        0,
        0xe728,
        &[vec![on(0, 0, 60, 100), off(10, 0, 60), end(0)]],
    );

    assert_eq!(MidiFile::read(&bytes), Err(MidiFileError::TimecodeTiming));
}

#[test]
fn a_file_without_notes_is_refused() {
    let bytes = smf(0, 96, &[vec![name("Empty"), end(384)]]);

    assert_eq!(MidiFile::read(&bytes), Err(MidiFileError::NoNotes));
}

#[test]
fn bytes_that_are_not_midi_are_refused() {
    assert!(matches!(
        MidiFile::read(b"RIFF\0\0\0\0WAVE"),
        Err(MidiFileError::Malformed(_))
    ));
}

#[test]
fn midi_paths_are_recognised_by_extension_in_any_case() {
    assert!(is_midi_file_path(Path::new("/tmp/Rebeater P3.mid")));
    assert!(is_midi_file_path(Path::new("song.MIDI")));
    assert!(!is_midi_file_path(Path::new("loop.wav")));
    assert!(!is_midi_file_path(Path::new("mid")));
}
