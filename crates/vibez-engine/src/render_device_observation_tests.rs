//! Offline observation boundaries retain rendered samples, failure precedence and warmed crops.

use super::*;
use crate::engine::OfflineRoutingSetup;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use vibez_core::routing::{RoutingChannel, RoutingEffect};

const RATE: u64 = 1;
const VALID: u64 = 2;
const LOCAL: u64 = 3;
const LATENCY: u64 = 4;
const REQUEST: u64 = 5;
const RECOVERY: u64 = 6;
const PROCESS: u64 = 7;
const ERROR: u64 = 8;

fn routing_channel(id: TrackId) -> RoutingChannel {
    RoutingChannel {
        id,
        is_bus: id.is_master(),
        effects: Vec::new(),
        sends: Vec::new(),
    }
}

fn routing_effect(id: EffectId) -> RoutingEffect {
    RoutingEffect {
        id,
        inputs: Vec::new(),
        assignments: Vec::new(),
        inactive_inputs: Vec::new(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Healthy,
    AtEntry,
    Requested,
    Invalid,
    Rate,
    Latency,
    Local,
    InstrumentRate,
    Error,
}

struct Trace {
    entries: [AtomicU64; 256],
    len: AtomicUsize,
    overflow: AtomicBool,
}

impl Trace {
    fn new() -> Self {
        Self {
            entries: std::array::from_fn(|_| AtomicU64::new(0)),
            len: AtomicUsize::new(0),
            overflow: AtomicBool::new(false),
        }
    }
    fn record(&self, owner: u64, method: u64, value: u32) {
        let index = self.len.fetch_add(1, Ordering::Relaxed);
        if let Some(entry) = self.entries.get(index) {
            entry.store(
                owner << 48 | method << 40 | u64::from(value),
                Ordering::Relaxed,
            );
        } else {
            self.overflow.store(true, Ordering::Relaxed);
        }
    }
    fn observed(&self) -> Vec<(u64, u64, u32)> {
        assert!(!self.overflow.load(Ordering::Relaxed));
        self.entries[..self.len.load(Ordering::Relaxed)]
            .iter()
            .map(|entry| {
                let value = entry.load(Ordering::Relaxed);
                (value >> 48, (value >> 40) & 255, value as u32)
            })
            .collect()
    }
    fn assert_boundary_observed(&self, mode: Mode) {
        let calls = self.observed();
        if matches!(mode, Mode::AtEntry | Mode::InstrumentRate) {
            let blocked = if mode == Mode::InstrumentRate {
                (1, RATE, 48_000)
            } else {
                (2, REQUEST, 1)
            };
            assert!(
                calls.contains(&blocked),
                "{mode:?}: the actual entry guard must observe the rejection"
            );
            assert!(!calls.iter().any(|(_, method, _)| *method == PROCESS));
            if mode == Mode::InstrumentRate {
                assert!(
                    calls.iter().all(|(owner, _, _)| *owner == 1),
                    "the invalid instrument must stop validation before querying its effects"
                );
            }
            return;
        }
        let processed = calls
            .iter()
            .position(|(owner, method, _)| *owner == 2 && *method == PROCESS)
            .unwrap();
        let changed = match mode {
            Mode::Requested => (REQUEST, 1),
            Mode::Invalid | Mode::Local => (VALID, 0),
            Mode::Rate => (RATE, 48_000),
            Mode::Latency => (LATENCY, 1),
            Mode::Error => (ERROR, 1),
            _ => (VALID, 1),
        };
        assert!(
            calls[processed + 1..].contains(&(2, changed.0, changed.1)),
            "{mode:?}: a later boundary must observe the real post-process state"
        );
    }
}

struct Facts {
    rate: AtomicU32,
    valid: AtomicBool,
    local: AtomicBool,
    latency: AtomicU32,
    requested: AtomicBool,
    error: AtomicBool,
    changed: AtomicBool,
    entered: AtomicBool,
    processes: AtomicUsize,
    frames: AtomicUsize,
}

impl Facts {
    fn new() -> Self {
        Self {
            rate: AtomicU32::new(0),
            valid: AtomicBool::new(true),
            local: AtomicBool::new(false),
            latency: AtomicU32::new(0),
            requested: AtomicBool::new(false),
            error: AtomicBool::new(false),
            changed: AtomicBool::new(false),
            entered: AtomicBool::new(false),
            processes: AtomicUsize::new(0),
            frames: AtomicUsize::new(0),
        }
    }
}

struct Probe {
    owner: u64,
    mode: Mode,
    facts: Arc<Facts>,
    trace: Arc<Trace>,
    memory: [f32; 2],
}

impl Probe {
    fn new(owner: u64, mode: Mode, facts: &Arc<Facts>, trace: &Arc<Trace>) -> Self {
        Self {
            owner,
            mode,
            facts: Arc::clone(facts),
            trace: Arc::clone(trace),
            memory: [0.0; 2],
        }
    }
}

macro_rules! observations {
    () => {
        fn activation_sample_rate(&self) -> Option<u32> {
            let value = self.facts.rate.load(Ordering::Acquire);
            self.trace.record(self.owner, RATE, value);
            (value != 0).then_some(value)
        }
        fn processing_configuration_valid(&self) -> bool {
            let value = self.facts.valid.load(Ordering::Acquire);
            self.trace.record(self.owner, VALID, u32::from(value));
            value
        }
        fn processing_failure_is_local(&self) -> bool {
            let value = self.facts.local.load(Ordering::Acquire);
            self.trace.record(self.owner, LOCAL, u32::from(value));
            value
        }
        fn latency_samples(&self) -> u32 {
            let value = self.facts.latency.load(Ordering::Acquire);
            self.trace.record(self.owner, LATENCY, value);
            value
        }
        fn reconfiguration_requested(&self) -> bool {
            if self.mode == Mode::AtEntry && !self.facts.entered.swap(true, Ordering::Relaxed) {
                self.facts.requested.store(true, Ordering::Release);
            }
            let value = self.facts.requested.load(Ordering::Acquire);
            self.trace.record(self.owner, REQUEST, u32::from(value));
            value
        }
        fn processing_recovery_requested(&self) -> bool {
            self.trace.record(self.owner, RECOVERY, 0);
            false
        }
        fn take_processing_error(&mut self) -> Option<&'static str> {
            let failed = self.facts.error.swap(false, Ordering::Relaxed);
            self.trace.record(self.owner, ERROR, u32::from(failed));
            failed.then_some("scripted offline process failure")
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
        fn reset(&mut self) {
            self.memory.fill(0.0);
        }
    };
}

impl vibez_dsp::effect::AudioEffect for Probe {
    observations!();
    fn effect_type(&self) -> EffectType {
        EffectType::Gain
    }
    fn process(&mut self, buffer: &mut [f32], channels: usize) {
        let frames = buffer.len() / channels;
        self.trace.record(self.owner, PROCESS, frames as u32);
        self.facts.processes.fetch_add(1, Ordering::Relaxed);
        self.facts.frames.fetch_add(frames, Ordering::Relaxed);
        for frame in buffer.chunks_exact_mut(channels) {
            for (channel, value) in frame.iter_mut().enumerate().take(2) {
                let next = *value * 0.5 + self.memory[channel] * 0.25;
                self.memory[channel] = next;
                *value = next;
            }
        }
        if self.facts.changed.swap(true, Ordering::Relaxed) {
            return;
        }
        match self.mode {
            Mode::Requested => self.facts.requested.store(true, Ordering::Release),
            Mode::Invalid => self.facts.valid.store(false, Ordering::Release),
            Mode::Rate => self.facts.rate.store(48_000, Ordering::Release),
            Mode::Latency => self.facts.latency.store(1, Ordering::Release),
            Mode::Local => {
                self.facts.local.store(true, Ordering::Release);
                self.facts.valid.store(false, Ordering::Release);
            }
            Mode::Error => {
                self.facts.error.store(true, Ordering::Release);
                self.facts.rate.store(48_000, Ordering::Release);
                self.facts.requested.store(true, Ordering::Release);
                self.facts.valid.store(false, Ordering::Release);
            }
            _ => {}
        }
    }
}

impl vibez_instruments::Instrument for Probe {
    observations!();
    fn instrument_kind(&self) -> InstrumentKind {
        InstrumentKind::SubtractiveSynth
    }
    fn note_on(&mut self, _: u8, _: u8) {}
    fn note_off(&mut self, _: u8) {}
    fn render(&mut self, buffer: &mut [f32], _: usize) {
        buffer.fill(0.25);
    }
}

fn normalize(error: Option<String>, track: TrackId, effect: EffectId) -> Option<String> {
    error.map(|text| {
        text.replace(&format!("channel {}", track.raw()), "channel <track>")
            .replace(&format!("Effect {} ", effect.raw()), "Effect <effect> ")
    })
}

fn expected_error(mode: Mode) -> Option<&'static str> {
    match mode {
        Mode::AtEntry | Mode::Requested | Mode::Invalid | Mode::Local => Some("Effect <effect> on channel <track> requires a valid processing configuration during Bounce"),
        Mode::Rate => Some("Effect <effect> on channel <track> was activated at 48000 Hz but Bounce requests 44100 Hz"),
        Mode::InstrumentRate => Some("Instrument on channel <track> was activated at 48000 Hz but Bounce requests 44100 Hz"),
        _ => None,
    }
}

fn reference(input: impl IntoIterator<Item = f32>) -> Vec<f32> {
    let mut previous = 0.0;
    input
        .into_iter()
        .map(|value| {
            let next = value * 0.5 + previous * 0.25;
            previous = next;
            next
        })
        .collect()
}

#[test]
fn isolated_offline_segments_sample_validation_entry_and_post_process_at_distinct_boundaries() {
    for mode in [
        Mode::Healthy,
        Mode::AtEntry,
        Mode::Requested,
        Mode::Invalid,
        Mode::Rate,
        Mode::Latency,
        Mode::Local,
        Mode::InstrumentRate,
    ] {
        let track = TrackId::new();
        let effect = EffectId::new();
        let trace = Arc::new(Trace::new());
        let instrument = Arc::new(Facts::new());
        let facts = Arc::new(Facts::new());
        if mode == Mode::InstrumentRate {
            instrument.rate.store(48_000, Ordering::Release);
            facts.requested.store(true, Ordering::Release);
        }
        let mut source = EngineTrack::new(track);
        source.pan = 0.0;
        source.instrument = Some(Box::new(Probe::new(1, Mode::Healthy, &instrument, &trace)));
        source.effects.push(EffectSlot {
            id: effect,
            effect: Box::new(Probe::new(2, mode, &facts, &trace)),
            bypass: false,
        });
        let mut channel = routing_channel(track);
        channel.effects.push(routing_effect(effect));
        let routing = crate::routing::PreparedRouting::prepare(
            &[channel, routing_channel(TrackId::MASTER)],
            8,
        )
        .unwrap();
        let mut engine = AudioEngine::for_offline_routing(OfflineRoutingSetup {
            sample_rate: 44_100,
            bpm: 120.0,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
            tracks: vec![Box::new(source)],
            buses: Vec::new(),
            master: EngineTrack::new(TrackId::MASTER),
            routing,
        });
        let mut output = [-123.0; 16];
        let mut selected = [-123.0; 16];
        assert_eq!(
            crate::retirement::tests::allocations(|| engine.render_offline_routing_segment(
                0,
                &mut output,
                Some((track, &mut selected))
            )),
            (0, 0)
        );
        assert_eq!(engine.pending_retirements.reserved(), 0);
        assert!(engine.pending_retirements.len() <= 32);
        let error = normalize(engine.offline_compensation_failure(), track, effect);
        assert_eq!(error.as_deref(), expected_error(mode));
        let blocked = matches!(mode, Mode::AtEntry | Mode::InstrumentRate);
        let expected_audio = reference([0.25; 8]);
        for (frame, values) in output.as_chunks::<2>().0.iter().enumerate() {
            assert_eq!(
                values[0],
                if blocked { 0.0 } else { expected_audio[frame] },
                "{mode:?}, frame{frame}"
            );
            assert_eq!(values[1], 0.0);
        }
        assert_eq!(output, selected);
        if mode == Mode::Latency {
            let mut second = vec![-123.0; 16];
            let mut second_selected = vec![-123.0; 16];
            assert_eq!(
                crate::retirement::tests::allocations(|| engine.render_offline_routing_segment(
                    8,
                    &mut second,
                    Some((track, &mut second_selected))
                )),
                (0, 0)
            );
            let second_error = normalize(engine.offline_compensation_failure(), track, effect);
            assert_eq!(
                second_error.as_deref(),
                Some("Applied audio configuration became invalid during Bounce")
            );
            assert!(second
                .iter()
                .chain(&second_selected)
                .all(|sample| *sample == 0.0));
        }
        trace.assert_boundary_observed(mode);
        assert_eq!(
            facts.processes.load(Ordering::Relaxed),
            usize::from(!blocked)
        );
        assert_eq!(engine.output_position_samples(), 0);
    }
}

fn request() -> (BounceRequest, EffectId) {
    let mut first = bare_track("Source");
    first.pan = 0.0;
    let mut second = bare_track("Other");
    second.pan = 0.0;
    let mut master = bare_track("Master");
    master.id = TrackId::MASTER;
    master.gain = 0.5;
    let effect = EffectId::new();
    first.effects.push(EffectInfo {
        id: effect,
        effect_type: EffectType::Gain,
        bypass: false,
        params: Vec::new(),
        plugin: Some(plugin_device("Observer")),
        sidechains: Vec::new(),
        inactive_sidechains: Default::default(),
    });
    let mut clips = Vec::new();
    let mut clip_audio = HashMap::new();
    for (index, track) in [&first, &second].into_iter().enumerate() {
        let id = ClipId::new();
        let signal: Vec<_> = (0..1024)
            .map(|frame| {
                if index == 0 {
                    (frame % 11) as f32 / 32.0 - 5.0 / 32.0
                } else {
                    0.125
                }
            })
            .collect();
        clip_audio.insert(
            id,
            Arc::new(DecodedAudio {
                channels: vec![signal.clone(), signal],
                sample_rate: 44_100,
            }),
        );
        clips.push(ClipInfo {
            id,
            track_id: track.id,
            name: "source".into(),
            position: 0,
            source_offset: 0,
            start_marker: None,
            duration: 1024,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            gain_db: Default::default(),
            fades: Default::default(),
            playback_direction: Default::default(),
            transient_markers: Default::default(),
            warp_markers: Default::default(),
            transpose: Default::default(),
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        });
    }
    (
        BounceRequest {
            tracks: vec![first, second],
            master: Some(master),
            buses: Vec::new(),
            audio_clips: clips,
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 900),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        },
        effect,
    )
}

#[test]
fn public_offline_full_and_cropped_master_and_track_outputs_keep_origin_warmed_history() {
    let (mut request, effect) = request();
    let track = request.tracks[0].id;
    let reference = reference((0..900).map(|frame| (frame % 11) as f32 / 32.0 - 5.0 / 32.0));
    for selected in [false, true] {
        let mut full = None;
        for (start, end) in [(0, 900), (521, 777)] {
            request.mode = if selected {
                BounceMode::Track(track)
            } else {
                BounceMode::Master
            };
            request.range_samples = (start, end);
            let trace = Arc::new(Trace::new());
            let facts = Arc::new(Facts::new());
            let mut plugins = OfflinePlugins::default();
            plugins.effects.insert(
                effect,
                Box::new(Probe::new(2, Mode::Healthy, &facts, &trace)),
            );
            let owner = crate::engine::reconfiguration::effect_owner_identity(
                &**plugins.effects.get(&effect).unwrap(),
            );
            let result = render_offline_with_plugins(&request, &mut plugins, |_| {}).unwrap();
            assert!(result.warnings.is_empty());
            assert_eq!(result.audio.sample_rate, 44_100);
            assert_eq!(
                crate::engine::reconfiguration::effect_owner_identity(
                    &**plugins.effects.get(&effect).unwrap()
                ),
                owner
            );
            for (offset, sample) in result.audio.channels[0].iter().enumerate() {
                let wet = reference[start as usize + offset];
                assert_eq!(*sample, if selected { wet } else { (wet + 0.125) * 0.5 });
            }
            assert!(result.audio.channels[1].iter().all(|sample| *sample == 0.0));
            assert_eq!(result.audio.num_frames(), (end - start) as usize);
            assert_eq!(facts.processes.load(Ordering::Relaxed), 2);
            assert_eq!(facts.frames.load(Ordering::Relaxed), end as usize);
            if let Some(previous) = &full {
                let previous: &Vec<Vec<f32>> = previous;
                for (channel, expected) in previous.iter().enumerate() {
                    assert_eq!(
                        result.audio.channels[channel],
                        expected[start as usize..end as usize]
                    );
                }
            } else {
                full = Some(result.audio.channels.clone());
            }
            let calls = trace.observed();
            let processing = calls
                .iter()
                .position(|(_, method, _)| *method == PROCESS)
                .unwrap();
            assert!(
                calls[..processing].contains(&(2, LATENCY, 0)),
                "latency reporting must precede rendering the owner's first real block"
            );
        }
    }
}

#[test]
fn public_offline_failures_keep_processing_error_precedence_and_original_stop_block() {
    for mode in [
        Mode::AtEntry,
        Mode::Requested,
        Mode::Invalid,
        Mode::Rate,
        Mode::Latency,
        Mode::Local,
        Mode::Error,
    ] {
        let (request, effect) = request();
        let track = request.tracks[0].id;
        let trace = Arc::new(Trace::new());
        let facts = Arc::new(Facts::new());
        let mut plugins = OfflinePlugins::default();
        plugins
            .effects
            .insert(effect, Box::new(Probe::new(2, mode, &facts, &trace)));
        let owner = crate::engine::reconfiguration::effect_owner_identity(
            &**plugins.effects.get(&effect).unwrap(),
        );
        let error = match render_offline_with_plugins(&request, &mut plugins, |_| {}) {
            Ok(_) => panic!("{mode:?} unexpectedly succeeded"),
            Err(error) => normalize(Some(error), track, effect).unwrap(),
        };
        assert_eq!(
            crate::engine::reconfiguration::effect_owner_identity(
                &**plugins.effects.get(&effect).unwrap()
            ),
            owner
        );
        let expected_error = if mode == Mode::Error {
            "Observer on channel 'Source' failed: scripted offline process failure"
        } else if mode == Mode::Latency {
            "Applied audio configuration became invalid during Bounce"
        } else {
            expected_error(mode).unwrap()
        };
        assert_eq!(error, expected_error, "{mode:?}");
        trace.assert_boundary_observed(mode);
        assert_eq!(
            facts.processes.load(Ordering::Relaxed),
            usize::from(mode != Mode::AtEntry)
        );
        assert_eq!(
            facts.frames.load(Ordering::Relaxed),
            if mode == Mode::AtEntry { 0 } else { 512 }
        );
    }
}
