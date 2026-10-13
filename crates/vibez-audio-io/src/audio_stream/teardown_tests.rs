//! Engine destruction must not depend on when CPAL releases its callback clone.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::ThreadId;
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::{EffectId, TrackId},
    routing::{RoutingChannel, RoutingEffect},
};
use vibez_dsp::effect::AudioEffect;
use vibez_engine::commands::EngineCommand;

struct Audit {
    processing: AtomicBool,
    destroyed: Mutex<Vec<(ThreadId, bool)>>,
}

struct Probe {
    audit: Arc<Audit>,
    entered: SyncSender<()>,
    release: Receiver<()>,
}

impl AudioEffect for Probe {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _: usize, _: f32) -> bool {
        false
    }
    fn get_param(&self, _: usize) -> f32 {
        0.0
    }
    fn reset(&mut self) {}
    fn process(&mut self, _: &mut [f32], _: usize) {
        self.audit.processing.store(true, Ordering::Release);
        self.entered.send(()).unwrap();
        self.release.recv().unwrap();
        self.audit.processing.store(false, Ordering::Release);
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        self.audit.destroyed.lock().unwrap().push((
            std::thread::current().id(),
            self.audit.processing.load(Ordering::Acquire),
        ));
    }
}

#[test]
fn teardown_extracts_engine_on_main_after_inflight_processing_and_before_late_callback() {
    let main = std::thread::current().id();
    let audit = Arc::new(Audit {
        processing: AtomicBool::new(false),
        destroyed: Mutex::new(Vec::new()),
    });
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let (late_tx, late_rx) = mpsc::sync_channel(1);
    let (engine, mut commands, _events) = AudioEngine::new();
    let track = TrackId::new();
    let effect = EffectId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Owner".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddPluginEffect {
            track_id: track,
            effect_id: effect,
            position: None,
            effect: Box::new(Probe {
                audit: Arc::clone(&audit),
                entered: entered_tx,
                release: release_rx,
            }),
        })
        .unwrap();
    let mut channel = RoutingChannel {
        id: track,
        is_bus: false,
        effects: vec![],
        sends: vec![],
    };
    channel.effects.push(RoutingEffect {
        id: effect,
        inputs: vec![],
        assignments: vec![],
        inactive_inputs: vec![],
    });
    let master = RoutingChannel {
        id: TrackId::MASTER,
        is_bus: true,
        effects: vec![],
        sends: vec![],
    };
    commands
        .push(EngineCommand::SetRouting(
            vibez_engine::routing::PreparedRouting::prepare(&[channel, master], 16).unwrap(),
        ))
        .unwrap();
    let stream = AudioOutputStream::idle(engine);
    let observer = Arc::clone(&stream.engine_slot);
    let callback_slot = Arc::clone(&stream.engine_slot);
    let callback =
        std::thread::spawn(move || {
            {
                let mut owner = callback_slot.lock().unwrap();
                owner.as_mut().unwrap().process_block(
                    vibez_engine::engine::AudioProcessBlock::new(&mut [0.0; 2], 2),
                );
            }
            late_rx.recv().unwrap();
            assert!(
                callback_slot.lock().unwrap().is_none(),
                "retired engine remains reachable to a late callback"
            );
        });
    entered_rx.recv().unwrap();
    let release = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(20));
        release_tx.send(()).unwrap();
    });
    drop(stream);
    let empty = observer.lock().unwrap().is_none();
    late_tx.send(()).unwrap();
    release.join().unwrap();
    let callback_result = callback.join();
    assert!(
        empty,
        "main teardown must extract the engine even while callback Arc clones remain"
    );
    callback_result.unwrap();
    assert_eq!(*audit.destroyed.lock().unwrap(), [(main, false)]);
}
