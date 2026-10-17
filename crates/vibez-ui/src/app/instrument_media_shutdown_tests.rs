//! Application shutdown must retain sample guards and state caches until the engine stops.

use super::*;
use std::sync::{Mutex, Weak};
use std::thread::ThreadId;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::effect::ParamDescriptor;
use vibez_core::id::TrackId;
use vibez_core::midi::InstrumentKind;
use vibez_core::track::DrumPadState;
use vibez_engine::engine::AudioProcessBlock;
use vibez_instruments::drum_rack::DrumRack;
use vibez_instruments::{Instrument, InstrumentMediaOwners, PreparedDrumPadState};

type DropObservations = Arc<Mutex<Vec<(ThreadId, usize)>>>;

struct DropObserverInstrument {
    inner: DrumRack,
    old_sample: Weak<DecodedAudio>,
    observations: DropObservations,
}

impl Drop for DropObserverInstrument {
    fn drop(&mut self) {
        self.observations
            .lock()
            .unwrap()
            .push((std::thread::current().id(), self.old_sample.strong_count()));
    }
}

impl Instrument for DropObserverInstrument {
    fn activation_sample_rate(&self) -> Option<u32> {
        self.inner.activation_sample_rate()
    }
    fn instrument_kind(&self) -> InstrumentKind {
        self.inner.instrument_kind()
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        self.inner.param_descriptors()
    }
    fn set_param(&mut self, index: usize, value: f32) -> bool {
        self.inner.set_param(index, value)
    }
    fn get_param(&self, index: usize) -> f32 {
        self.inner.get_param(index)
    }
    fn note_on(&mut self, pitch: u8, velocity: u8) {
        self.inner.note_on(pitch, velocity);
    }
    fn note_off(&mut self, pitch: u8) {
        self.inner.note_off(pitch);
    }
    fn render(&mut self, output: &mut [f32], channels: usize) {
        self.inner.render(output, channels);
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
    fn stop_processing(&mut self) {
        self.inner.stop_processing();
    }
    fn load_sample(&mut self, sample: Arc<DecodedAudio>, name: String) -> InstrumentMediaOwners {
        self.inner.load_sample(sample, name)
    }
    fn load_drum_pad_sample(
        &mut self,
        pad: usize,
        sample: Arc<DecodedAudio>,
        name: String,
    ) -> InstrumentMediaOwners {
        self.inner.load_drum_pad_sample(pad, sample, name)
    }
    fn clear_drum_pad(&mut self, pad: usize) -> InstrumentMediaOwners {
        self.inner.clear_drum_pad(pad)
    }
    fn set_drum_pad_state(
        &mut self,
        pad: usize,
        state: PreparedDrumPadState,
    ) -> InstrumentMediaOwners {
        self.inner.set_drum_pad_state(pad, state)
    }
}

#[test]
fn actual_app_shutdown_stops_engine_before_destroying_sample_guards_and_state_cache() {
    let owner_thread = std::thread::current().id();
    let observations: DropObservations = Arc::new(Mutex::new(Vec::new()));
    let old = Arc::new(DecodedAudio {
        channels: vec![vec![0.25; 4096]],
        sample_rate: 1000,
    });
    let old_sample = Arc::downgrade(&old);
    let mut app = test_support::app();
    app.state.browser.waveform_audio = Some(Arc::clone(&old));
    let (mut engine, mut commands, events) = AudioEngine::new();
    app.event_rx = Some(events);
    let track = TrackId::new();
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::add_track(track)).unwrap();
    commands
        .push(EngineCommand::SetRouting(
            vibez_engine::routing::PreparedRouting::prepare(
                &[track, TrackId::MASTER].map(|id| vibez_core::routing::RoutingChannel {
                    id,
                    is_bus: id.is_master(),
                    effects: vec![],
                    sends: vec![],
                }),
                vibez_engine::routing::MAX_BLOCK_FRAMES,
            )
            .unwrap(),
        ))
        .unwrap();
    commands
        .push(EngineCommand::SetInstrument {
            track_id: track,
            instrument: Box::new(DropObserverInstrument {
                inner: DrumRack::new(1000.0),
                old_sample: old_sample.clone(),
                observations: Arc::clone(&observations),
            }),
        })
        .unwrap();
    commands
        .push(EngineCommand::set_drum_rack_pad_state(
            track,
            0,
            DrumPadState {
                fade_in_ms: 0.0,
                fade_out_ms: 0.0,
                one_shot: false,
                ..Default::default()
            },
        ))
        .unwrap();
    commands
        .push(EngineCommand::LoadDrumRackPadSample {
            track_id: track,
            pad_index: 0,
            sample: old,
            sample_name: "old-still-sounding.wav".into(),
        })
        .unwrap();
    commands
        .push(EngineCommand::ExternalNoteOn {
            track_id: track,
            pitch: 36,
            velocity: 127,
        })
        .unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 1));
    // A pad from legacy settings sits 3 dB down in a mono render.
    assert_eq!(output, [0.25 * std::f32::consts::FRAC_1_SQRT_2; 16]);
    app.poll_engine_events();
    commands
        .push(EngineCommand::LoadDrumRackPadSample {
            track_id: track,
            pad_index: 0,
            sample: Arc::new(DecodedAudio {
                channels: vec![vec![0.5; 4096]],
                sample_rate: 1000,
            }),
            sample_name: "replacement-not-yet-triggered.wav".into(),
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut output, 1));
    // A pad's new sample stops that pad's voices.
    assert_eq!(output, [0.0; 16]);
    app.poll_engine_events();
    assert_eq!(
        old_sample.strong_count(),
        2,
        "actual App guard and state cache"
    );
    assert!(observations.lock().unwrap().is_empty());
    app._stream = Some(AudioOutputStream::idle(engine));
    drop(app);
    assert_eq!(
        observations.lock().unwrap().as_slice(),
        &[(owner_thread, 2)],
        "stream shutdown must precede both automatic state and guard destruction"
    );
    assert_eq!(old_sample.strong_count(), 0);
}
