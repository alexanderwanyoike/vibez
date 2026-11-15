//! Bouncing a MIDI From receiver renders its source's instrument first and routes its notes.

use super::*;
use vibez_core::routing::SourceTap;

fn routed_request() -> (BounceRequest, TrackId) {
    let mut source = bare_track("Rebeater");
    source.kind = TrackKind::Midi;
    source.plugin_instrument = Some(plugin_device("Rebeater"));
    let mut receiver = bare_track("Lead");
    receiver.kind = TrackKind::Midi;
    receiver.midi_from = vibez_core::midi_from::MidiFrom::new(receiver.id, source.id);
    let receiver_id = receiver.id;
    let source_id = source.id;
    let request = BounceRequest {
        tracks: vec![receiver, source],
        master: None,
        buses: vec![],
        audio_clips: vec![],
        note_clips: vec![],
        clip_audio: HashMap::new(),
        sampler_audio: HashMap::new(),
        drum_pad_audio: HashMap::new(),
        mode: BounceMode::Track(receiver_id),
        range_samples: (0, 4096),
        bpm: 120.0,
        sample_rate: 44100,
        swing: vibez_core::perform::SwingAmount::STRAIGHT,
    };
    (request, source_id)
}

#[test]
fn a_receiver_bounce_renders_its_sources_instrument_and_routes_its_notes() {
    let (request, source) = routed_request();

    assert!(potential_dependency_ids(&request).contains(&source));
    let dependencies = super::dependencies::actual_dependencies(&request, None);
    assert_eq!(
        dependencies.taps.get(&source),
        Some(&SourceTap::BeforeEffects)
    );
    let receiving = dependencies
        .graph_channels
        .iter()
        .find(|channel| channel.midi_from.is_some())
        .unwrap();
    assert_eq!(receiving.midi_from, Some(source));
}

#[test]
fn a_receiver_bounce_needs_its_sources_plugin() {
    let (request, source) = routed_request();
    let mut plugins = OfflinePlugins::default();
    plugins
        .instrument_failures
        .insert(source, "Missing library".into());

    let error = render_offline_with_plugins(&request, &mut plugins, |_| {})
        .err()
        .unwrap();

    assert!(error.contains("Missing library"));
}
