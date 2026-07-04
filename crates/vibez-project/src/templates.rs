//! Genre starter projects.
//!
//! Each template returns a [`Project`] with a tempo, an empty track layout,
//! and default instrument assignments. Templates intentionally ship no audio
//! content: they're a shape to pour samples into, not a pre-composed song.

use vibez_core::constants::DEFAULT_SAMPLE_RATE;
use vibez_core::midi::{InstrumentKind, TrackKind};
use vibez_core::track::{DrumPadState, InstrumentStateInfo, TrackInfo};

use crate::Project;

/// Identifier + display metadata for a genre template.
pub struct GenreTemplate {
    pub id: &'static str,
    pub name: &'static str,
    pub bpm: f64,
    build_fn: fn() -> Project,
}

impl GenreTemplate {
    pub fn build(&self) -> Project {
        (self.build_fn)()
    }
}

pub const TEMPLATES: &[GenreTemplate] = &[
    GenreTemplate {
        id: "house",
        name: "House",
        bpm: 124.0,
        build_fn: house,
    },
    GenreTemplate {
        id: "techno",
        name: "Techno",
        bpm: 130.0,
        build_fn: techno,
    },
    GenreTemplate {
        id: "trance",
        name: "Trance",
        bpm: 138.0,
        build_fn: trance,
    },
    GenreTemplate {
        id: "dnb",
        name: "Drum & Bass",
        bpm: 174.0,
        build_fn: dnb,
    },
    GenreTemplate {
        id: "ukg",
        name: "UK Garage",
        bpm: 130.0,
        build_fn: ukg,
    },
    GenreTemplate {
        id: "big_beat",
        name: "Big Beat",
        bpm: 110.0,
        build_fn: big_beat,
    },
    GenreTemplate {
        id: "electro",
        name: "Electro",
        bpm: 128.0,
        build_fn: electro,
    },
];

pub fn find(id: &str) -> Option<&'static GenreTemplate> {
    TEMPLATES.iter().find(|t| t.id == id)
}

fn empty_pads() -> Vec<DrumPadState> {
    (0..16)
        .map(|_| DrumPadState {
            source: None,
            gain: 1.0,
            pan: 0.0,
            start: 0.0,
            end: 1.0,
            coarse_tune: 0,
            fine_tune: 0.0,
            one_shot: true,
            choke_group: None,
        })
        .collect()
}

fn drum_track(name: &str, color: u8) -> TrackInfo {
    let mut t = TrackInfo::new(name);
    t.kind = TrackKind::Instrument(InstrumentKind::DrumRack);
    t.instrument = Some(InstrumentKind::DrumRack);
    t.native_instrument = Some(InstrumentStateInfo::DrumRack { pads: empty_pads() });
    t.color_index = color;
    t
}

fn synth_track(name: &str, color: u8) -> TrackInfo {
    let mut t = TrackInfo::new(name);
    t.kind = TrackKind::Instrument(InstrumentKind::SubtractiveSynth);
    t.instrument = Some(InstrumentKind::SubtractiveSynth);
    t.native_instrument = Some(InstrumentStateInfo::SubtractiveSynth { params: Vec::new() });
    t.color_index = color;
    t
}

fn sampler_track(name: &str, color: u8) -> TrackInfo {
    let mut t = TrackInfo::new(name);
    t.kind = TrackKind::Instrument(InstrumentKind::Sampler);
    t.instrument = Some(InstrumentKind::Sampler);
    t.native_instrument = Some(InstrumentStateInfo::Sampler {
        params: Vec::new(),
        source: None,
    });
    t.color_index = color;
    t
}

fn audio_track(name: &str, color: u8) -> TrackInfo {
    let mut t = TrackInfo::new(name);
    t.kind = TrackKind::Audio;
    t.color_index = color;
    t
}

fn midi_track(name: &str, color: u8) -> TrackInfo {
    let mut t = TrackInfo::new(name);
    t.kind = TrackKind::Midi;
    t.color_index = color;
    t
}

fn base(name: &str, bpm: f64, tracks: Vec<TrackInfo>) -> Project {
    Project {
        name: name.into(),
        bpm,
        sample_rate: DEFAULT_SAMPLE_RATE,
        tracks,
        clips: Vec::new(),
        note_clips: Vec::new(),
    }
}

fn house() -> Project {
    base(
        "House",
        124.0,
        vec![
            drum_track("Drums", 0),
            drum_track("Perc", 1),
            synth_track("Bass", 2),
            synth_track("Chord Stab", 3),
            sampler_track("Vox Chop", 4),
            audio_track("Top Loop", 5),
            audio_track("FX", 6),
        ],
    )
}

fn techno() -> Project {
    base(
        "Techno",
        130.0,
        vec![
            drum_track("Drums", 0),
            drum_track("Perc", 1),
            synth_track("Bass", 2),
            synth_track("Stab", 3),
            sampler_track("Riser", 4),
            audio_track("Atmo", 5),
            audio_track("FX", 6),
        ],
    )
}

fn trance() -> Project {
    base(
        "Trance",
        138.0,
        vec![
            drum_track("Drums", 0),
            drum_track("Hats/Perc", 1),
            synth_track("Bass", 2),
            synth_track("Lead", 3),
            synth_track("Pad", 4),
            sampler_track("Pluck", 5),
            audio_track("Atmo", 6),
            audio_track("FX", 7),
        ],
    )
}

fn dnb() -> Project {
    base(
        "Drum & Bass",
        174.0,
        vec![
            drum_track("Kicks", 0),
            drum_track("Snares", 1),
            drum_track("Breaks/Hats", 2),
            synth_track("Reese", 3),
            synth_track("Sub", 4),
            sampler_track("Vocal", 5),
            audio_track("Atmo", 6),
            audio_track("FX", 7),
        ],
    )
}

fn ukg() -> Project {
    base(
        "UK Garage",
        130.0,
        vec![
            drum_track("Drums", 0),
            drum_track("Perc/Shuffle", 1),
            synth_track("Bass", 2),
            sampler_track("Vocal Chop", 3),
            synth_track("Stab", 4),
            audio_track("Top Loop", 5),
            audio_track("FX", 6),
        ],
    )
}

fn big_beat() -> Project {
    base(
        "Big Beat",
        110.0,
        vec![
            drum_track("Breakbeat", 0),
            drum_track("Perc", 1),
            synth_track("Bass", 2),
            synth_track("Lead", 3),
            sampler_track("Vocal Hit", 4),
            audio_track("Guitar/Loop", 5),
            audio_track("FX", 6),
        ],
    )
}

fn electro() -> Project {
    base(
        "Electro",
        128.0,
        vec![
            drum_track("Drums", 0),
            drum_track("Perc", 1),
            synth_track("Bass", 2),
            synth_track("Lead", 3),
            synth_track("Pad", 4),
            midi_track("Seq", 5),
            audio_track("FX", 6),
        ],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_templates_build() {
        for t in TEMPLATES {
            let project = t.build();
            assert!((project.bpm - t.bpm).abs() < f64::EPSILON, "{}", t.id);
            assert!(!project.tracks.is_empty(), "{}", t.id);
            assert!(project.clips.is_empty());
            assert!(project.note_clips.is_empty());
        }
    }

    #[test]
    fn house_has_expected_shape() {
        let p = house();
        assert!((p.bpm - 124.0).abs() < 1e-9);
        assert!(p.tracks.iter().any(|t| t.name == "Drums"));
        assert!(p.tracks.iter().any(|t| t.name == "Bass"));
    }

    #[test]
    fn drum_rack_templates_have_16_pads() {
        for t in TEMPLATES {
            let project = t.build();
            for track in &project.tracks {
                if let Some(InstrumentStateInfo::DrumRack { pads }) = &track.native_instrument {
                    assert_eq!(pads.len(), 16, "{} / {}", t.id, track.name);
                }
            }
        }
    }

    #[test]
    fn find_by_id() {
        assert!(find("house").is_some());
        assert!(find("nonexistent").is_none());
    }
}
