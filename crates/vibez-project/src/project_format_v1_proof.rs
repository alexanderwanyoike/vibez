use super::*;

/// Representative non-media document used by the repeatable proof and its
/// tests. It includes MIDI, automation, native-device state and opaque
/// third-party plugin state.
pub fn representative_document() -> ProjectDocumentV1 {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    use vibez_core::effect::{EffectInfo, EffectType, PluginDeviceInfo};
    use vibez_core::id::{ClipId, EffectId};
    use vibez_core::midi::{MidiNote, NoteClipInfo};
    use vibez_core::track::{InstrumentStateInfo, TrackInfo};

    let mut track = TrackInfo::new("Proof instrument");
    track.native_instrument = Some(InstrumentStateInfo::SubtractiveSynth {
        params: vec![0.05, 0.2, 0.8, 0.4],
    });
    track.effects.push(EffectInfo {
        sidechains: Default::default(),

        id: EffectId::new(),
        effect_type: EffectType::Gain,
        bypass: false,
        params: vec![1.0],
        plugin: Some(PluginDeviceInfo {
            format: "clap".into(),
            uid: "org.vibez.proof-plugin".into(),
            path: PathBuf::from("/plugins/proof.clap"),
            name: "Opaque Proof Plugin".into(),
            state_b64: Some("AAECA/7/UGx1Z2luU3RhdGU=".into()),
        }),
    });
    let mut automation = AutomationLane::new(AutomationTarget::TrackGain);
    automation.insert_point(AutomationPoint {
        beat: 0.0,
        value: 0.25,
        curve: 0.0,
    });
    automation.insert_point(AutomationPoint {
        beat: 16.0,
        value: 0.9,
        curve: 0.35,
    });
    track.automation.push(automation);

    let track_id = track.id;
    let project = Project {
        name: "Project Format V1 proof".into(),
        bpm: 126.0,
        groove_profile: vibez_core::perform::GrooveProfile::default(),
        swing: vibez_core::perform::SwingAmount::default(),
        sample_rate: 48_000,
        tracks: vec![track],
        arrange: crate::TimelineInfo {
            note_clips: vec![NoteClipInfo {
                id: ClipId::new(),
                track_id,
                name: "Proof pattern".into(),
                position_beats: 0.0,
                duration_beats: 4.0,
                notes: vec![
                    MidiNote {
                        pitch: 36,
                        velocity: 112,
                        start_beat: 0.0,
                        duration_beats: 0.25,
                    },
                    MidiNote {
                        pitch: 42,
                        velocity: 96,
                        start_beat: 0.5,
                        duration_beats: 0.125,
                    },
                ],
                start_marker_beats: None,
                loop_enabled: true,
                loop_start_beats: 0.0,
                loop_end_beats: 4.0,
                groove_grid: vibez_core::perform::GrooveGrid::Off,
            }],
            ..crate::TimelineInfo::default()
        },
        master: Some(TrackInfo::new("Master")),
        buses: vec![TrackInfo::new("Return A")],
        sections: Vec::new(),
        perform_layout: Default::default(),
        launcher_clips: Vec::new(),
    };
    ProjectDocumentV1::new(project)
}
