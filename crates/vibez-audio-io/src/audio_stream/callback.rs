//! Real-time CPAL callback adapter and sample-format conversion.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::audio_input::AudioInputBridge;
use cpal::traits::DeviceTrait;
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig, SupportedBufferSize};
use vibez_engine::engine::{AudioEngine, AudioProcessBlock};

use crate::stream_config::StreamOpenError;

use super::{AudioStreamError, AudioStreamEvent, StreamEventReporter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CallbackAction {
    Process,
    SilenceAndYield,
}

#[derive(Clone, Default)]
pub(super) struct StreamHealth(Arc<AtomicBool>);

impl StreamHealth {
    pub(super) fn mark_failed(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub(super) fn callback_action(&self) -> CallbackAction {
        if self.0.load(Ordering::Acquire) {
            CallbackAction::SilenceAndYield
        } else {
            CallbackAction::Process
        }
    }
}

/// Smoothed fraction of each device buffer deadline spent in the audio
/// callback, stored as basis points so both threads remain lock-free.
#[derive(Clone, Default)]
pub(super) struct StreamCpuLoad(Arc<AtomicU32>);

impl StreamCpuLoad {
    pub(super) fn record(&self, elapsed: Duration, frames: usize, sample_rate: u32) {
        let instantaneous = callback_load_basis_points(elapsed, frames, sample_rate);
        let previous = self.0.load(Ordering::Relaxed);
        let smoothed = if previous == 0 {
            instantaneous
        } else {
            (previous.saturating_mul(4) + instantaneous) / 5
        };
        self.0.store(smoothed, Ordering::Relaxed);
    }

    pub(super) fn percent(&self) -> f32 {
        self.0.load(Ordering::Relaxed) as f32 / 100.0
    }
}

pub(super) struct OutputCallback {
    engine_slot: Arc<Mutex<Option<AudioEngine>>>,
    health: StreamHealth,
    cpu_load: StreamCpuLoad,
    channels: usize,
    buffer_frames: u32,
    scratch_frames: u32,
    sample_rate: u32,
    rt_state: Option<Result<audio_thread_priority::RtPriorityHandle, ()>>,
    input_bridge: Arc<AudioInputBridge>,
    input_scratch: Vec<f32>,
    resample_scratch: Vec<f32>,
}

impl OutputCallback {
    pub(super) fn new(
        engine_slot: Arc<Mutex<Option<AudioEngine>>>,
        cpu_load: StreamCpuLoad,
        channels: usize,
        buffer_frames: u32,
        scratch_frames: u32,
        sample_rate: u32,
        input_bridge: Arc<AudioInputBridge>,
    ) -> Self {
        Self {
            engine_slot,
            health: StreamHealth::default(),
            cpu_load,
            channels,
            buffer_frames,
            scratch_frames,
            sample_rate,
            rt_state: None,
            input_bridge,
            input_scratch: vec![0.0; scratch_frames as usize * channels],
            resample_scratch: vec![0.0; scratch_frames as usize * channels],
        }
    }

    fn process(&mut self, data: &mut [f32]) {
        if self.health.callback_action() == CallbackAction::SilenceAndYield {
            if let Some(Ok(handle)) = self.rt_state.take() {
                if let Err(error) =
                    audio_thread_priority::demote_current_thread_from_real_time(handle)
                {
                    eprintln!("vibez: failed to demote disconnected audio thread: {error}");
                }
            }
            data.fill(0.0);
            // ALSA can hot-loop callbacks after a USB device disappears.
            std::thread::sleep(Duration::from_millis(10));
            return;
        }
        if self.rt_state.is_none() {
            self.rt_state = Some(
                match audio_thread_priority::promote_current_thread_to_real_time(
                    self.buffer_frames,
                    self.sample_rate,
                ) {
                    Ok(handle) => {
                        eprintln!("vibez: audio thread promoted to realtime");
                        Ok(handle)
                    }
                    Err(error) => {
                        eprintln!("vibez: failed to promote audio thread: {error}");
                        Err(())
                    }
                },
            );
        }

        let started = Instant::now();
        let mut processed = false;
        if let Ok(mut guard) = self.engine_slot.try_lock() {
            if let Some(engine) = guard.as_mut() {
                self.input_bridge
                    .latch_record_start_position(engine.arrangement_position_samples());
                let live_input = self.input_scratch.get_mut(..data.len()).map(|scratch| {
                    let target = self.input_bridge.clock_output(scratch, self.channels);
                    (target, &*scratch)
                });
                if live_input.is_none() {
                    self.input_bridge.report_overflow();
                }
                let resample_source = self.input_bridge.resample_source_track_raw();
                let resample_scratch = self.resample_scratch.get_mut(..data.len());
                if resample_source.is_some() && resample_scratch.is_none() {
                    self.input_bridge.report_overflow();
                }
                let mut block = AudioProcessBlock::new(data, self.channels);
                if let Some((Some(target), input)) = live_input {
                    block = block.with_live_input(target, input);
                }
                if let (Some(source), Some(capture)) = (resample_source, resample_scratch) {
                    block = block.with_track_output_capture(source, capture);
                }
                engine.process_block(block);
                if resample_source.is_some() {
                    if let Some(capture) = self.resample_scratch.get(..data.len()) {
                        self.input_bridge
                            .capture_track_output(capture, self.channels);
                    }
                }
                processed = true;
            }
        }
        if !processed {
            data.fill(0.0);
        }
        self.cpu_load.record(
            started.elapsed(),
            data.len() / self.channels,
            self.sample_rate,
        );
    }
}

const CONSERVATIVE_SCRATCH_FRAMES: u32 = 16_384;

/// Preallocate conversion scratch on the UI thread. Some backends report an
/// unbounded maximum, so cap that hint at a generous callback size while never
/// shrinking below the explicitly requested buffer.
pub(super) fn scratch_buffer_frames(
    requested_frames: Option<u32>,
    supported: &SupportedBufferSize,
) -> u32 {
    let requested = requested_frames.unwrap_or(512);
    let reported = match supported {
        SupportedBufferSize::Range { max, .. } => (*max).min(CONSERVATIVE_SCRATCH_FRAMES),
        SupportedBufferSize::Unknown => CONSERVATIVE_SCRATCH_FRAMES,
    };
    requested.max(reported).max(1)
}

pub(super) fn build_output_stream_for_format(
    device: &cpal::Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    callback: OutputCallback,
    event_reporter: StreamEventReporter,
) -> Result<cpal::Stream, AudioStreamError> {
    macro_rules! build {
        (f32) => {
            build_f32_stream(device, config, callback, event_reporter)?
        };
        ($sample:ty) => {
            build_converting_stream::<$sample>(device, config, callback, event_reporter)?
        };
    }
    let stream = dispatch_sample_format!(sample_format, build, |format| {
        AudioStreamError::from(StreamOpenError::Unsupported(format!(
            "audio output sample format {format} is not supported"
        )))
    });
    Ok(stream)
}

fn build_f32_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    mut callback: OutputCallback,
    event_reporter: StreamEventReporter,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let health = callback.health.clone();
    device.build_output_stream(
        config,
        move |data: &mut [f32], _info| callback.process(data),
        move |error| report_stream_error(&health, &event_reporter, error),
        None,
    )
}

fn build_converting_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut callback: OutputCallback,
    event_reporter: StreamEventReporter,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + FromSample<f32>,
{
    let mut scratch = vec![0.0; callback.scratch_frames as usize * callback.channels];
    let health = callback.health.clone();
    device.build_output_stream(
        config,
        move |data: &mut [T], _info| {
            // A backend violating its advertised maximum must not force an
            // allocation on the real-time thread. Silence that callback.
            let Some(scratch) = scratch.get_mut(..data.len()) else {
                data.fill(T::from_sample(0.0));
                return;
            };
            callback.process(&mut *scratch);
            for (output, sample) in data.iter_mut().zip(scratch.iter()) {
                *output = T::from_sample(*sample);
            }
        },
        move |error| report_stream_error(&health, &event_reporter, error),
        None,
    )
}

fn report_stream_error(
    health: &StreamHealth,
    reporter: &StreamEventReporter,
    error: cpal::StreamError,
) {
    health.mark_failed();
    reporter.report(AudioStreamEvent::Error(error.to_string()));
    eprintln!("vibez: audio stream error: {error}");
}

pub(super) fn callback_load_basis_points(
    elapsed: Duration,
    frames: usize,
    sample_rate: u32,
) -> u32 {
    if frames == 0 || sample_rate == 0 {
        return 0;
    }
    let deadline_seconds = frames as f64 / sample_rate as f64;
    ((elapsed.as_secs_f64() / deadline_seconds) * 10_000.0)
        .round()
        .clamp(0.0, 99_900.0) as u32
}
