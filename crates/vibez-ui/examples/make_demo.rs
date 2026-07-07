//! Generate the demo project shipped in `assets/` (used for the
//! README screenshot). Run: cargo run -p vibez-ui --example make_demo

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{InstrumentKind, MidiNote, NoteClipInfo, TrackKind};
use vibez_core::track::TrackInfo;
use vibez_project::Project;

fn note(pitch: u8, velocity: u8, start_beat: f64, duration_beats: f64) -> MidiNote {
    MidiNote {
        pitch,
        velocity,
        start_beat,
        duration_beats,
    }
}

fn track(name: &str, kind: InstrumentKind, color_index: u8) -> TrackInfo {
    TrackInfo {
        id: TrackId::new(),
        name: name.to_string(),
        gain: 0.9,
        pan: 0.5,
        mute: false,
        solo: false,
        effects: Vec::new(),
        kind: TrackKind::Midi,
        color_index,
        instrument: Some(kind),
        native_instrument: None,
        plugin_instrument: None,
        automation: Vec::new(),
    }
}

fn clip(
    track_id: TrackId,
    name: &str,
    position_beats: f64,
    duration_beats: f64,
    notes: Vec<MidiNote>,
) -> NoteClipInfo {
    NoteClipInfo {
        id: ClipId::new(),
        track_id,
        name: name.to_string(),
        position_beats,
        duration_beats,
        notes,
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
    }
}

fn drum_bar(bar: f64, with_snare: bool) -> Vec<MidiNote> {
    let mut notes = Vec::new();
    let b = bar * 4.0;
    for i in 0..4 {
        notes.push(note(36, 110, b + i as f64, 0.5)); // kick on quarters
    }
    if with_snare {
        notes.push(note(38, 100, b + 1.0, 0.5));
        notes.push(note(38, 100, b + 3.0, 0.5));
    }
    for i in 0..8 {
        let v = if i % 2 == 0 { 72 } else { 52 };
        notes.push(note(42, v, b + i as f64 * 0.5, 0.25)); // closed hats
    }
    notes
}

fn main() {
    let drums = track("Drums", InstrumentKind::DrumRack, 0);
    let bass = track("Bass", InstrumentKind::SubtractiveSynth, 2);
    let chords = track("Chords", InstrumentKind::SubtractiveSynth, 4);
    let lead = track("Lead", InstrumentKind::SubtractiveSynth, 6);

    let mut note_clips = Vec::new();

    // Drums: intro without snare, then full kit.
    let mut intro = Vec::new();
    for bar in 0..4 {
        intro.extend(drum_bar(bar as f64, false));
    }
    note_clips.push(clip(drums.id, "Kick + Hats", 0.0, 16.0, intro));
    for (pos, name) in [(16.0, "Full Kit"), (32.0, "Full Kit"), (48.0, "Full Kit")] {
        let mut bars = Vec::new();
        for bar in 0..4 {
            bars.extend(drum_bar(bar as f64, true));
        }
        note_clips.push(clip(drums.id, name, pos, 16.0, bars));
    }

    // Bass: A minor 16th groove, roots follow Am / F / C / G.
    let roots = [33u8, 29, 24, 31]; // A1 F1 C1 G1
    for section in 0..3 {
        let mut notes = Vec::new();
        for (bar, root) in roots.iter().enumerate() {
            let b = bar as f64 * 4.0;
            for step in [0.0, 0.75, 1.5, 2.0, 2.75, 3.5] {
                notes.push(note(*root, 96, b + step, 0.45));
            }
            notes.push(note(root + 12, 84, b + 3.75, 0.2));
        }
        note_clips.push(clip(
            bass.id,
            "Bassline",
            16.0 + section as f64 * 16.0,
            16.0,
            notes,
        ));
    }

    // Chords: Am F C G pads, one chord per bar.
    let voicings: [&[u8]; 4] = [
        &[57, 60, 64], // Am
        &[53, 57, 60], // F
        &[48, 60, 64], // C (open low root)
        &[55, 59, 62], // G
    ];
    for pos in [16.0, 48.0] {
        let mut notes = Vec::new();
        for (bar, chord) in voicings.iter().enumerate() {
            for pitch in *chord {
                notes.push(note(*pitch, 78, bar as f64 * 4.0, 3.8));
            }
        }
        note_clips.push(clip(chords.id, "Pads", pos, 16.0, notes));
    }

    // Lead: A minor pentatonic hook.
    let hook = [
        (69, 0.0, 0.75),
        (72, 1.0, 0.5),
        (76, 1.5, 0.5),
        (74, 2.0, 1.0),
        (72, 3.0, 0.5),
        (69, 3.5, 0.5),
        (67, 4.0, 1.5),
        (69, 6.0, 1.75),
        (76, 8.0, 0.75),
        (79, 9.0, 0.5),
        (76, 9.5, 0.5),
        (74, 10.0, 1.0),
        (72, 11.0, 0.5),
        (74, 11.5, 0.5),
        (69, 12.0, 3.5),
    ];
    let notes = hook.iter().map(|(p, s, d)| note(*p, 92, *s, *d)).collect();
    note_clips.push(clip(lead.id, "Hook", 32.0, 16.0, notes));

    let project = Project {
        name: "Neon Skyline".to_string(),
        bpm: 124.0,
        sample_rate: 48_000,
        tracks: vec![drums, bass, chords, lead],
        clips: Vec::new(),
        note_clips,
    };
    let path = std::path::Path::new("assets/demo.vibez");
    project.save_to_file(path).expect("save demo project");
    println!("wrote {}", path.display());
}
