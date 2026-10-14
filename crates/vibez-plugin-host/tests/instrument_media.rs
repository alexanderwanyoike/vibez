//! Hosted instruments return unsupported native media without callback destruction.

#[allow(dead_code)]
mod support;
#[global_allocator]
static ALLOCATOR: support::allocation::AllocationCounter = support::allocation::AllocationCounter;

use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_instruments::Instrument;
use vibez_plugin_host::PluginInstrumentWrapper;

#[test]
fn loadable_plugin_wrappers_return_unsupported_sample_owners_unchanged() {
    let fixture = support::Fixture::new();
    for format in ["clap", "vst3"] {
        let mut instrument = PluginInstrumentWrapper::new(fixture.load_instrument(format, 64));
        let sample = Arc::new(DecodedAudio { channels: vec![vec![0.25; 128]], sample_rate: 48_000 });
        let pointer = Arc::as_ptr(&sample);
        let name = String::from("native-sample-is-not-for-this-plugin.wav");
        let name_pointer = name.as_ptr();
        let mut returned = None;
        assert_eq!(support::allocation::count_activity(|| {
            returned = Some(instrument.load_sample(sample, name));
        }), (0, 0), "{format}");
        let returned = returned.unwrap();
        assert_eq!(returned.sample.as_ref().map(Arc::as_ptr), Some(pointer));
        assert_eq!(returned.name.as_ref().map(|name| name.as_ptr()), Some(name_pointer));
        assert_eq!(instrument.sample_name(), None);
        drop(returned);
    }
}
