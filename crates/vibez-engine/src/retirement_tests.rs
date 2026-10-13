use super::*;
use crate::{
    commands::EngineCommand,
    engine::{AudioEngine, AudioProcessBlock},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    sync::{
        atomic::{AtomicBool, AtomicUsize},
        OnceLock,
    },
};
use vibez_core::{
    effect::{EffectType, ParamDescriptor},
    id::{EffectId, TrackId},
};

thread_local! {static COUNTING:Cell<bool>=const {Cell::new(false)};static ALLOCATIONS:Cell<usize>=const {Cell::new(0)};static DEALLOCATIONS:Cell<usize>=const {Cell::new(0)};}
struct Counter;
#[global_allocator]
static ALLOCATOR: Counter = Counter;
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let _ = COUNTING.try_with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        let _ = COUNTING.try_with(|active| {
            if active.get() {
                DEALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.dealloc(pointer, layout)
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let _ = COUNTING.try_with(|active| {
            if active.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        System.realloc(pointer, layout, size)
    }
}
pub(crate) fn allocations(action: impl FnOnce()) -> (usize, usize) {
    ALLOCATIONS.with(|count| count.set(0));
    DEALLOCATIONS.with(|count| count.set(0));
    COUNTING.with(|active| active.set(true));
    action();
    COUNTING.with(|active| active.set(false));
    (ALLOCATIONS.with(Cell::get), DEALLOCATIONS.with(Cell::get))
}
struct Probe {
    audio_thread: Arc<OnceLock<std::thread::ThreadId>>,
    stop_on_audio: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
    drop_on_audio: Arc<AtomicBool>,
}
impl vibez_dsp::effect::AudioEffect for Probe {
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn param_descriptors(&self) -> &'static [ParamDescriptor] {
        &[]
    }
    fn set_param(&mut self, _index: usize, _value: f32) -> bool {
        false
    }
    fn get_param(&self, _index: usize) -> f32 {
        0.0
    }
    fn process(&mut self, _buffer: &mut [f32], _channels: usize) {}
    fn reset(&mut self) {}
    fn stop_processing(&mut self) {
        self.stop_on_audio.store(
            self.audio_thread.get() == Some(&std::thread::current().id()),
            Ordering::Relaxed,
        );
        self.stopped.store(true, Ordering::Release);
    }
}
impl Drop for Probe {
    fn drop(&mut self) {
        self.drop_on_audio.store(
            self.audio_thread.get() == Some(&std::thread::current().id()),
            Ordering::Relaxed,
        );
        self.drops.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn source_and_effect_removal_stop_on_audio_and_drop_on_ui_without_callback_allocation() {
    for remove_channel in [false, true] {
        let (mut engine, mut commands, mut events) = AudioEngine::new();
        let track = TrackId::new();
        let effect = EffectId::new();
        let audio_thread = Arc::new(OnceLock::new());
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_on_audio = Arc::new(AtomicBool::new(false));
        let drops = Arc::new(AtomicUsize::new(0));
        let drop_on_audio = Arc::new(AtomicBool::new(false));
        commands
            .push(EngineCommand::AddTrack(track, "Source".into()))
            .unwrap();
        commands
            .push(EngineCommand::AddPluginEffect {
                track_id: track,
                effect_id: effect,
                effect: Box::new(Probe {
                    audio_thread: Arc::clone(&audio_thread),
                    stopped: Arc::clone(&stopped),
                    stop_on_audio: Arc::clone(&stop_on_audio),
                    drops: Arc::clone(&drops),
                    drop_on_audio: Arc::clone(&drop_on_audio),
                }),
                position: None,
            })
            .unwrap();
        let (mut engine, count) = std::thread::spawn(move || {
            audio_thread.set(std::thread::current().id()).unwrap();
            engine.process_block(AudioProcessBlock::new(&mut [], 2));
            commands
                .push(if remove_channel {
                    EngineCommand::RemoveTrack(track)
                } else {
                    EngineCommand::RemoveEffect(track, effect)
                })
                .unwrap();
            let count = allocations(|| engine.process_block(AudioProcessBlock::new(&mut [], 2)));
            (engine, count)
        })
        .join()
        .unwrap();
        assert_eq!(count, (0, 0));
        assert!(stopped.load(Ordering::Acquire));
        assert!(stop_on_audio.load(Ordering::Relaxed));
        assert_eq!(drops.load(Ordering::Relaxed), 0);
        while let Ok(event) = events.pop() {
            drop(event);
        }
        assert_eq!(drops.load(Ordering::Relaxed), 1);
        assert!(!drop_on_audio.load(Ordering::Relaxed));
        engine.flush_retirements();
    }
}

#[test]
fn full_event_ring_keeps_channel_owner_until_retirement_can_be_delivered() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Source".into()))
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    commands.push(EngineCommand::RemoveTrack(track)).unwrap();
    assert_eq!(
        allocations(|| engine.process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    assert_eq!(engine.pending_retirements.len(), 1);
    assert!(engine
        .channel_retirement
        .slots
        .iter()
        .any(|slot| slot.occupied.load(Ordering::Acquire)));
    while events.pop().is_ok() {}
    engine.flush_retirements();
    assert!(engine.pending_retirements.is_empty());
    let mut retired = 0;
    while let Ok(event) = events.pop() {
        if matches!(event, EngineEvent::RetiredChannel(_)) {
            retired += 1;
        }
        drop(event);
    }
    assert_eq!(retired, 1);
    assert!(engine
        .channel_retirement
        .slots
        .iter()
        .all(|slot| !slot.occupied.load(Ordering::Acquire)));
}

#[test]
fn bus_automation_cleanup_retires_point_storage_without_callback_destruction() {
    use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let track = TrackId::new();
    let bus = TrackId::new();
    commands
        .push(EngineCommand::AddTrack(track, "Track".into()))
        .unwrap();
    commands
        .push(EngineCommand::AddBus(bus, "Bus".into()))
        .unwrap();
    let mut lane = AutomationLane::new(AutomationTarget::Send { bus_id: bus });
    lane.points.push(AutomationPoint {
        beat: 0.0,
        value: 1.0,
        curve: 0.0,
    });
    commands
        .push(EngineCommand::SetAutomationLane {
            track_id: track,
            lane,
        })
        .unwrap();
    engine.process_block(AudioProcessBlock::new(&mut [], 2));
    while engine
        .event_tx
        .push(EngineEvent::PlaybackPosition(0))
        .is_ok()
    {}
    commands.push(EngineCommand::RemoveBus(bus)).unwrap();
    assert_eq!(
        allocations(|| engine.process_block(AudioProcessBlock::new(&mut [], 2))),
        (0, 0)
    );
    assert!(engine.tracks()[0].sends.is_empty());
    assert!(engine.tracks()[0].playback_source.automation.is_empty());
    assert_eq!(engine.pending_retirements.len(), 2);
    while events.pop().is_ok() {}
    engine.flush_retirements();
    let mut lanes = 0;
    while let Ok(event) = events.pop() {
        if matches!(event, EngineEvent::RetiredAutomationLane(_)) {
            lanes += 1;
        }
        drop(event);
    }
    assert_eq!(lanes, 1);
}
