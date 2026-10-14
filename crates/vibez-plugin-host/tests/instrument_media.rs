//! Hosted instruments return unsupported native media without callback destruction.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::{DrumPadState, MediaSourceRef};
use vibez_instruments::{Instrument, PreparedDrumPadState};
use vibez_plugin_host::PluginInstrumentWrapper;

#[test]
fn loadable_plugin_wrappers_return_all_unsupported_native_media_owners_unchanged() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instrument = PluginInstrumentWrapper::new(fixture.load_instrument(format, 64));
        for method in ["load_sample", "load_drum_pad_sample", "set_drum_pad_state", "clear_drum_pad"] {
            let sample = matches!(method, "load_sample" | "load_drum_pad_sample").then(|| Arc::new(DecodedAudio {
                channels: vec![vec![0.25; 128]],
                sample_rate: 48_000,
            }));
            let pointer = sample.as_ref().map(Arc::as_ptr);
            let weak = sample.as_ref().map(Arc::downgrade);
            let mut name = (method != "clear_drum_pad").then(|| format!("unsupported-{method}.wav"));
            let name_pointer = name.as_ref().map(|name| name.as_ptr());
            let state = (method == "set_drum_pad_state").then(|| PreparedDrumPadState::from(DrumPadState {
                name: name.take(),
                source: Some(MediaSourceRef::LocalFile { path: "/unused/native-source.wav".into() }),
                gain: 0.37,
                ..Default::default()
            }));
            let mut returned = None;
            assert_eq!(
                support::allocation::count_activity(|| {
                    returned = Some(match method {
                        "load_sample" => instrument.load_sample(sample.unwrap(), name.unwrap()),
                        "load_drum_pad_sample" => instrument.load_drum_pad_sample(usize::MAX, sample.unwrap(), name.unwrap()),
                        "set_drum_pad_state" => instrument.set_drum_pad_state(usize::MAX, state.unwrap()),
                        _ => instrument.clear_drum_pad(usize::MAX),
                    });
                }),
                (0, 0),
                "{format}, {method}"
            );
            let returned = returned.unwrap();
            assert_eq!(returned.sample.as_ref().map(Arc::as_ptr), pointer);
            assert_eq!(returned.name.as_ref().map(|name| name.as_ptr()), name_pointer);
            assert_eq!(returned.is_empty(), method == "clear_drum_pad");
            assert_eq!(instrument.sample_name(), None);
            if let Some(weak) = &weak {
                assert_eq!(weak.strong_count(), 1);
            }
            drop(returned);
            if let Some(weak) = weak {
                assert_eq!(weak.strong_count(), 0);
            }
        }
    }
}
