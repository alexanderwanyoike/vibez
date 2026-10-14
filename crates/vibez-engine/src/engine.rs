//! Real-time project playback, prepared ownership and callback clock coordination.

use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
#[cfg(test)]
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::RING_BUFFER_CAPACITY;
use vibez_core::id::{SectionId, TrackId};
use vibez_core::perform::{GrooveProfile, SwingAmount};

use vibez_core::time::TempoMap;

use crate::commands::{AuditionStart, EngineCommand};
use crate::engine_audition::{audition_fade_frames, next_audition_boundary, AuditionBus};
use crate::events::EngineEvent;
use crate::metering;
use crate::mixer::{
    any_solo, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip, EngineTrack,
    InstrumentRenderContext,
};
use crate::note_repeat::{NoteRepeatClock, NoteRepeatStart};
use crate::playback_source::{calculate_total_length, PreparedSectionPlaybackSource};
use crate::transport::Transport;

use self::clock::ClockDomain;

/// Capacity of the UI spectrum-analyser sample ring: roughly a third
/// of a second of mono audio at 48 kHz, ample for a 60 fps drain.
const SPECTRUM_RING_CAPACITY: usize = 16_384;

/// The rate a new engine runs at until the first `SetSampleRate`.
pub const DEFAULT_SAMPLE_RATE: u32 = 44_100;

/// The real-time audio engine.
///
/// `AudioEngine` lives on the audio thread. Its
/// [`process_block()`](AudioEngine::process_block) method is called once per
/// audio callback to fill an output buffer with audio and communicate with the
/// UI thread via lock-free ring buffers.
///
/// # Construction
///
/// Use [`AudioEngine::new()`] which returns the engine together with the
/// "other ends" of the ring buffers that the UI thread should hold:
///
/// ```ignore
/// let (engine, cmd_tx, event_rx) = AudioEngine::new();
/// // Move `engine` to the audio thread.
/// // Keep `cmd_tx` and `event_rx` on the UI thread.
/// ```
pub struct AudioEngine {
    scheduled_presentation: Vec<presentation_queue::ScheduledPresentation>,
    section_capture_timing: Vec<presentation::SectionCaptureTiming>,
    capture_replay: Option<presentation::CaptureReplay>,
    presentation_state: presentation_state::PresentationState,
    capture_lifecycle: capture_lifecycle::CaptureLifecycle,
    pending_source_cleanup: bool,
    compensation_callback_heard_start: u64,
    capture_clip_positions: Vec<(TrackId, u64, u64)>,
    device_handoff: device_handoff::DeviceHandoffState,
    compensation_plan: compensation_plan::CompensationPlanState,
    output_gate: output_gate::OutputGate,
    routing: Box<crate::routing::PreparedRouting>,
    project_routing_published: bool,
    // The first publication has no public retirement event. Keep its bounded
    // bootstrap owner until the engine returns to the caller for teardown.
    bootstrap_routing: Option<Box<crate::routing::PreparedRouting>>,
    retired_routing: Option<Box<crate::routing::PreparedRouting>>,
    pub(super) pending_retirements: Vec<EngineEvent>,
    pub(super) pending_bus_cleanup: Option<(TrackId, usize)>,
    pub(super) channel_retirement: Arc<crate::retirement::ChannelRetirementPool>,
    transport: Transport,
    /// Multi-track state.
    tracks: crate::mixer::ChannelList,
    /// The master bus: a track-shaped channel (gain + effect chain,
    /// no clips or instrument) applied to the summed mix. Addressed
    /// by commands via [`TrackId::MASTER`].
    master: Box<EngineTrack>,
    /// Return channels: track-shaped, fed by per-track post-fader
    /// sends, summed into the mix before the master chain.
    buses: crate::mixer::ChannelList,
    /// Which track streams samples to the UI spectrum analyser.
    spectrum_track: Option<TrackId>,
    /// Producer side of the spectrum ring (mono samples).
    spectrum_tx: Producer<f32>,
    /// Consumer side, parked here until the UI takes it before the
    /// engine moves to the audio thread.
    spectrum_rx: Option<Consumer<f32>>,
    /// One-shot Browser audition after project master processing.
    audition: AuditionBus,
    sample_rate: u32,
    cmd_rx: Consumer<EngineCommand>,
    pub(super) event_tx: Producer<EngineEvent>,
    clip_event_drops: u64,
    reported_clip_event_drops: u64,
    clip_resync_track: Option<usize>,
    clip_through_request: u64,
    /// Set when process_multitrack split the block at the arrangement
    /// loop boundary; suppresses the post-advance discontinuity flush
    /// for that block (segment-2 notes are legitimately sounding).
    split_wrap_handled: bool,
    active_section: Option<ActiveSectionPlayback>,
    clip_performance: bool,
    pending_clip_batch: Option<clip_batch::PendingClipBatch>,
    queued_section: Option<QueuedSectionPlayback>,
    pending_section_record: Option<section_record::PendingSectionRecord>,
    active_section_record: Option<section_record::ActiveSectionRecord>,
    /// When a transition lands partway through the current callback, only
    /// these frames advance the newly active Section's local playhead.
    section_advance_override: Option<u64>,
    arrangement_audio_length: Option<u64>,
    /// Audio Track Recording may extend Arrange beyond existing content.
    arrangement_recording: bool,
    /// Project Swing is engine state because generated-event consumers share
    /// it while existing playback remains untouched.
    project_swing: SwingAmount,
    /// Continues Note Repeat timing while transport is stopped. While playing,
    /// this follows the active clock domain.
    performance_position: u64,
    output_position: u64,
    rendered_callback_frames: usize,
    clip_record: Option<clip_record::ClipRecordRuntime>,
    /// The clock currently authorised to advance. Perform owns an independent
    /// zero-based engine timeline; its playback must never mutate the
    /// canonical Arrange cursor held by `transport.position()`.
    clock_domain: ClockDomain,
    /// Vibez extends MPC Note Repeat to stopped transport. The first held pad
    /// establishes this shared musical downbeat until the last repeat stops.
    stopped_note_repeat_anchor: Option<u64>,
}

#[derive(Debug, Clone, Copy)]
struct ActiveSectionPlayback {
    section_id: SectionId,
    position_samples: u64,
    length_samples: u64,
    looping: bool,
}

struct QueuedSectionPlayback {
    prepared: Box<PreparedSectionPlaybackSource>,
    effective_at_samples: u64,
}

#[derive(Clone, Copy)]
pub(super) struct LiveInputBlock<'a> {
    pub target_track_raw: u64,
    pub samples: &'a [f32],
}

/// One callback-lifetime buffer receiving a Project Track's direct audible
/// output. The caller preallocates the slice; the engine never retains it.
pub(super) struct TrackOutputCapture<'a> {
    pub source_track_raw: u64,
    pub samples: &'a mut [f32],
}

/// All transient sources and destinations for one audio callback.
///
/// The builder keeps callback-only borrows together so adding another optional
/// source does not multiply the public `AudioEngine` process API.
pub struct AudioProcessBlock<'a> {
    output: &'a mut [f32],
    channels: usize,
    live_input: Option<LiveInputBlock<'a>>,
    track_output_capture: Option<TrackOutputCapture<'a>>,
}

impl<'a> AudioProcessBlock<'a> {
    pub fn new(output: &'a mut [f32], channels: usize) -> Self {
        Self {
            output,
            channels,
            live_input: None,
            track_output_capture: None,
        }
    }

    pub fn with_live_input(mut self, target_track_raw: u64, samples: &'a [f32]) -> Self {
        self.live_input = Some(LiveInputBlock {
            target_track_raw,
            samples,
        });
        self
    }

    pub fn with_track_output_capture(
        mut self,
        source_track_raw: u64,
        samples: &'a mut [f32],
    ) -> Self {
        self.track_output_capture = Some(TrackOutputCapture {
            source_track_raw,
            samples,
        });
        self
    }
}

impl<'a> LiveInputBlock<'a> {
    pub(super) fn slice(self, frame_offset: usize, frames: usize, channels: usize) -> Self {
        let start = frame_offset.saturating_mul(channels);
        let end = start
            .saturating_add(frames.saturating_mul(channels))
            .min(self.samples.len());
        Self {
            target_track_raw: self.target_track_raw,
            samples: self.samples.get(start..end).unwrap_or(&[]),
        }
    }
}

impl AudioEngine {
    /// Timing profile compiled into V1 generated-event scheduling.
    pub const fn groove_profile() -> GrooveProfile {
        GrooveProfile::Mpc2000XlV1
    }

    /// Create a new audio engine.
    ///
    /// Returns `(engine, command_producer, event_consumer)`.  The caller
    /// should move `engine` to the audio thread and keep the producer /
    /// consumer on the UI thread.
    pub fn new() -> (Self, Producer<EngineCommand>, Consumer<EngineEvent>) {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCommand>::new(RING_BUFFER_CAPACITY);
        let (event_tx, event_rx) = RingBuffer::<EngineEvent>::new(RING_BUFFER_CAPACITY);
        let (spectrum_tx, spectrum_rx) = RingBuffer::<f32>::new(SPECTRUM_RING_CAPACITY);

        let engine = Self {
            routing: crate::routing::PreparedRouting::prepare(
                &[vibez_core::routing::RoutingChannel {
                    id: TrackId::MASTER,
                    is_bus: true,
                    effects: vec![],
                    sends: vec![],
                }],
                crate::routing::MAX_BLOCK_FRAMES,
            )
            .expect("static master routing is valid"),
            project_routing_published: false,
            bootstrap_routing: None,
            scheduled_presentation: Vec::with_capacity(
                presentation_queue::PRESENTATION_EVENT_CAPACITY,
            ),
            section_capture_timing: Vec::with_capacity(
                presentation_queue::PRESENTATION_EVENT_CAPACITY,
            ),
            capture_replay: None,
            presentation_state: presentation_state::PresentationState::default(),
            capture_lifecycle: capture_lifecycle::CaptureLifecycle::default(),
            pending_source_cleanup: false,
            compensation_callback_heard_start: 0,
            capture_clip_positions: Vec::with_capacity(
                presentation_queue::PRESENTATION_EVENT_CAPACITY,
            ),
            device_handoff: device_handoff::DeviceHandoffState::default(),
            compensation_plan: compensation_plan::CompensationPlanState::default(),
            output_gate: output_gate::OutputGate::default(),
            retired_routing: None,
            pending_bus_cleanup: None,
            pending_retirements: Vec::with_capacity(crate::retirement::RETIREMENT_CAPACITY),
            channel_retirement: crate::retirement::ChannelRetirementPool::new(),
            transport: Transport::new(),
            tracks: Vec::with_capacity(crate::mixer::CHANNEL_CAPACITY),
            master: Box::new(EngineTrack::new(TrackId::MASTER)),
            buses: Vec::with_capacity(crate::mixer::CHANNEL_CAPACITY),
            spectrum_track: None,
            spectrum_tx,
            spectrum_rx: Some(spectrum_rx),
            audition: AuditionBus::new(),
            sample_rate: DEFAULT_SAMPLE_RATE,
            cmd_rx,
            event_tx,
            clip_event_drops: 0,
            reported_clip_event_drops: 0,
            clip_resync_track: None,
            clip_through_request: 0,
            split_wrap_handled: false,
            active_section: None,
            clip_performance: false,
            pending_clip_batch: None,
            queued_section: None,
            pending_section_record: None,
            active_section_record: None,
            section_advance_override: None,
            arrangement_audio_length: None,
            arrangement_recording: false,
            project_swing: SwingAmount::default(),
            performance_position: 0,
            output_position: 0,
            rendered_callback_frames: 0,
            clip_record: None,
            clock_domain: ClockDomain::Arrange,
            stopped_note_repeat_anchor: None,
        };

        (engine, cmd_tx, event_rx)
    }

    /// Take the consumer side of the spectrum-analyser ring. Must be
    /// called on the UI thread before the engine moves to the audio
    /// thread; returns `None` if already taken.
    pub fn take_spectrum_consumer(&mut self) -> Option<Consumer<f32>> {
        self.spectrum_rx.take()
    }

    /// Process one audio callback worth of data.
    ///
    /// `output` is an interleaved stereo buffer (`[L0, R0, L1, R1, ...]`)
    /// that must be filled with audio.  `channels` is the number of
    /// interleaved channels (typically 2).
    ///
    /// This method is **lock-free and allocation-free**.  It:
    /// 1. Drains all pending commands from the ring buffer.
    /// 2. Zeros the output buffer.
    /// 3. Renders project sources, effects and channels.
    /// 4. Sends metering and position events to the UI thread.
    #[cfg(test)]
    fn process(&mut self, output: &mut [f32], channels: usize) {
        self.process_block(AudioProcessBlock::new(output, channels));
    }

    /// Output frames advance even while stopped, so count-in audio can be trimmed exactly.
    pub fn output_position_samples(&self) -> u64 {
        self.output_position
    }

    /// Exact Arrange cursor at the next output-clock block boundary.
    pub fn arrangement_position_samples(&self) -> u64 {
        self.transport.position()
    }

    pub fn process_block(&mut self, block: AudioProcessBlock<'_>) {
        let AudioProcessBlock {
            output,
            channels,
            live_input,
            mut track_output_capture,
        } = block;
        self.compensation_callback_heard_start = self.heard_capture_position();
        self.flush_processing_errors();
        self.resync_clip_events();
        // A musical boundary due at this block start owns the same timestamp
        // as commands drained below. Publish the recording start first so a
        // first pad strike at the boundary cannot reach consumers while the
        // take still appears armed.
        self.apply_clip_record_boundary(self.performance_position);
        self.start_section_record_if_due(self.performance_position);

        // ---- 1. Drain commands ------------------------------------------
        self.drain_commands();
        self.rendered_callback_frames = 0;
        self.begin_device_reconfiguration();
        self.validate_compensation();

        let frames = output.len().checked_div(channels).unwrap_or(0);

        // ---- 2. Zero output buffer --------------------------------------
        output.iter_mut().for_each(|s| *s = 0.0);
        if let Some(capture) = track_output_capture.as_mut() {
            capture.samples.fill(0.0);
        }

        self.process_multitrack(
            output,
            frames,
            channels,
            live_input,
            track_output_capture.as_mut(),
        );

        self.validate_compensation();
        let project_muted = self.project_processing_muted();
        if project_muted {
            output.fill(0.0);
            if let Some(capture) = track_output_capture.as_mut() {
                capture.samples.fill(0.0);
            }
        }
        self.apply_compensation_transition(output, channels, project_muted);
        self.flush_processing_errors();

        // ---- 4.5 Audition Bus (post-master, outside project graph) ------
        self.audition.process(
            output,
            frames,
            channels,
            &self.transport,
            self.sample_rate,
            &mut self.event_tx,
        );

        // ---- 5. Advance transport and send events -----------------------
        let was_playing = self.transport.is_playing();
        let pos_before = self.transport.position();
        let performing = self.clock_domain == ClockDomain::Perform;
        let new_pos = if performing {
            self.transport.position()
        } else {
            self.transport.advance(frames as u64)
        };
        self.performance_position = if was_playing {
            if performing {
                self.performance_position.saturating_add(frames as u64)
            } else {
                new_pos
            }
        } else {
            self.performance_position.saturating_add(frames as u64)
        };

        let mut section_ended = false;
        let mut section_end_boundary = self.performance_position;
        if was_playing {
            if let Some(section) = self.active_section.as_mut() {
                let section_frames = self
                    .section_advance_override
                    .take()
                    .unwrap_or(frames as u64);
                let advanced = section.position_samples.saturating_add(section_frames);
                if section.looping && section.length_samples > 0 {
                    section.position_samples = advanced % section.length_samples;
                } else {
                    section.position_samples = advanced.min(section.length_samples);
                    if advanced >= section.length_samples {
                        section_ended = true;
                        section_end_boundary = self
                            .performance_position
                            .saturating_sub(advanced.saturating_sub(section.length_samples));
                    }
                }
                let presented = (self.routing.compensation.output_latency > 0)
                    .then(|| {
                        self.routing
                            .presentation
                            .audible()
                            .or(self.routing.presentation_start)
                    })
                    .flatten();
                let (section_id, position_samples) = presented
                    .and_then(|position| {
                        position
                            .section_id
                            .zip(position.section)
                            .map(|(id, local)| {
                                (
                                    id,
                                    if position.section_length > 0 {
                                        local.saturating_add(1) % position.section_length
                                    } else {
                                        local
                                    },
                                )
                            })
                    })
                    .unwrap_or((section.section_id, section.position_samples));
                let _ = self.event_tx.push(EngineEvent::SectionPlaybackPosition {
                    section_id,
                    position_samples,
                });
            }
        }
        if section_ended {
            for track in &mut self.tracks {
                track.flush_notes();
            }
        }
        // A queued Section owns the next boundary: the finished one-shot
        // stays silent on the Perform clock until the queue takes over.
        if section_ended && self.queued_section.is_none() {
            self.stop_section_record();
            self.apply_end_of_section_track_mutes_at_queued_boundary();
            self.cancel_queued_track_mutes();
            let rendered = self.rendered_callback_frames;
            self.rendered_callback_frames = frames.saturating_sub(
                self.performance_position
                    .saturating_sub(section_end_boundary) as usize,
            );
            self.section_capture_stopped(section_end_boundary);
            self.capture_lifecycle.end();
            self.present_event_after(
                EngineEvent::PerformanceCaptureStopped {
                    effective_at_samples: section_end_boundary,
                },
                self.mix_latency(),
            );
            self.rendered_callback_frames = rendered;
            self.transport.stop();
            self.rendered_callback_frames = frames.saturating_sub(
                self.performance_position
                    .saturating_sub(section_end_boundary) as usize,
            );
            self.present_event_after(EngineEvent::PlaybackStopped, self.mix_latency());
            self.rendered_callback_frames = rendered;
            self.active_section = None;
            self.clock_domain = ClockDomain::Arrange;
            self.transport
                .set_audio_length(self.arrangement_audio_length);
        }

        // Discontinuous jump NOT already handled by the loop-boundary
        // split (auto-stop at project end, loop region moved behind
        // the playhead): kill sounding notes, otherwise they hang
        // forever because the note schedulers only reason about
        // adjacent positions. When the split ran, notes started
        // after the wrap are legitimately sounding and must survive.
        if was_playing
            && !performing
            && !self.split_wrap_handled
            && new_pos != pos_before.saturating_add(frames as u64)
        {
            for track in &mut self.tracks {
                track.flush_notes();
            }
        }
        self.split_wrap_handled = false;

        // Position event.
        let audible = (self.routing.compensation.output_latency > 0)
            .then(|| {
                self.routing
                    .presentation
                    .audible()
                    .map(|position| (position, true))
                    .or_else(|| {
                        self.routing
                            .presentation_start
                            .map(|position| (position, false))
                    })
            })
            .flatten();
        let audible_arrange = audible.map_or(new_pos, |(position, advanced)| {
            position
                .arrange
                .saturating_add(u64::from(advanced && !performing))
        });
        let audible_arrange = if was_playing {
            audible_arrange
        } else {
            audible_arrange.min(new_pos)
        };
        let _ = self
            .event_tx
            .push(EngineEvent::PlaybackPosition(audible_arrange));
        if performing {
            let _ = self.event_tx.push(EngineEvent::PerformancePosition(
                audible.map_or(self.performance_position, |(position, advanced)| {
                    position.perform.saturating_add(u64::from(advanced))
                }),
            ));
        }

        if self.active_section.is_none() {
            if let Some(position) = self.routing.presentation.audible() {
                if let Some((section_id, local)) = position.section_id.zip(position.section) {
                    let _ = self.event_tx.push(EngineEvent::SectionPlaybackPosition {
                        section_id,
                        position_samples: local.saturating_add(1).min(position.section_length),
                    });
                    let _ = self.event_tx.push(EngineEvent::PerformancePosition(
                        position.perform.saturating_add(1),
                    ));
                }
            }
        }
        self.output_position = self.output_position.saturating_add(frames as u64);
        self.rendered_callback_frames = 0;
        self.flush_presentation();
        self.continue_capture_replay();
        self.flush_presentation();
        self.retire_section_capture_timing();

        // Master metering event.
        let meters = metering::calculate_meters(output, channels);
        let _ = self.event_tx.push(EngineEvent::Metering {
            peak_l: meters.peak_l,
            peak_r: meters.peak_r,
            rms_l: meters.rms_l,
            rms_r: meters.rms_r,
        });
    }

    /// Read the current transport (for inspection / testing).
    pub fn transport(&self) -> &Transport {
        &self.transport
    }

    /// Read the tracks (for inspection / testing).
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub(super) fn tracks_mut_for_retirement(&mut self) -> &mut [Box<EngineTrack>] {
        &mut self.tracks
    }

    pub fn tracks(&self) -> &[Box<EngineTrack>] {
        &self.tracks
    }

    fn reschedule_note_repeats(&mut self) {
        let position = self.performance_position;
        let bpm = self.transport.bpm();
        let sample_rate = self.sample_rate;
        let project_swing = self.project_swing;
        for track in &mut self.tracks {
            track.reschedule_note_repeats(position, bpm, sample_rate, project_swing);
        }
    }

    fn has_active_note_repeats(&self) -> bool {
        self.tracks
            .iter()
            .any(|track| track.has_active_note_repeats())
    }

    fn playing_note_repeat_anchor(&self) -> u64 {
        self.active_section
            .map(|section| {
                self.performance_position
                    .saturating_sub(section.position_samples)
            })
            .unwrap_or(0)
    }

    fn reanchor_note_repeats(&mut self, anchor_sample: u64, after_sample: u64) {
        let bpm = self.transport.bpm();
        let sample_rate = self.sample_rate;
        let project_swing = self.project_swing;
        for track in &mut self.tracks {
            track.reanchor_note_repeats(
                anchor_sample,
                after_sample,
                bpm,
                sample_rate,
                project_swing,
            );
        }
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Resolve a command's target channel: the master for
    /// [`TrackId::MASTER`], otherwise a track or a bus. Only used by
    /// commands that make sense on all of them (gain, pan, mute,
    /// effect chain).
    fn channel_mut(&mut self, id: TrackId) -> Option<&mut EngineTrack> {
        if id.is_master() {
            return Some(&mut self.master);
        }
        if let Some(track) = self.tracks.iter_mut().find(|t| t.id == id) {
            return Some(track);
        }
        self.buses
            .iter_mut()
            .find(|b| b.id == id)
            .map(|bus| &mut **bus)
    }
}

/// Stream a block to the UI spectrum analyser as mono samples.
/// Lock-free and allocation-free; drops samples when the ring is
/// full (the UI drains at 60 fps, so sustained overflow just means
/// the analyser skips audio it would have averaged away anyway).
fn push_spectrum(tx: &mut Producer<f32>, buffer: &[f32], channels: usize) {
    if channels >= 2 {
        for frame in buffer.chunks_exact(channels) {
            let _ = tx.push((frame[0] + frame[1]) * 0.5);
        }
    } else {
        for &s in buffer {
            let _ = tx.push(s);
        }
    }
}

#[path = "engine_drain_commands.rs"]
mod drain_commands;

#[path = "engine_automation_commands.rs"]
mod automation_commands;

#[path = "engine_render.rs"]
mod render_paths;

#[path = "engine_presentation.rs"]
mod presentation;

#[path = "engine_clock.rs"]
mod clock;

#[path = "engine_section_queue.rs"]
mod section_queue;

#[path = "engine_section_record.rs"]
pub(crate) mod section_record;

#[path = "engine_clip_record.rs"]
mod clip_record;

#[cfg(test)]
#[path = "engine_graph_callback_tests.rs"]
mod graph_callback_tests;

#[cfg(test)]
#[path = "engine_default_routing_tests.rs"]
mod default_routing_tests;

#[cfg(test)]
#[path = "engine_compensation_state_test_support.rs"]
mod compensation_state_test_support;

#[cfg(test)]
#[path = "engine_compensation_gate_state_tests.rs"]
mod compensation_gate_state_tests;

#[cfg(test)]
#[path = "engine_presentation_state_tests.rs"]
mod presentation_state_tests;

#[cfg(test)]
#[path = "engine_compensation_state_tests.rs"]
mod compensation_state_tests;

#[cfg(test)]
#[path = "engine_device_handoff_state_tests.rs"]
mod device_handoff_state_tests;

#[cfg(test)]
#[path = "engine_device_handoff_full_tests.rs"]
mod device_handoff_full_tests;

#[cfg(test)]
#[path = "engine_device_handoff_stale_tests.rs"]
mod device_handoff_stale_tests;

#[cfg(test)]
#[path = "engine_device_handoff_parameter_tests.rs"]
mod device_handoff_parameter_tests;

#[cfg(test)]
#[path = "engine_graph_test_support.rs"]
mod graph_test_support;

#[cfg(test)]
#[path = "engine_graph_harness_tests.rs"]
mod graph_harness_tests;

#[cfg(test)]
#[path = "engine_stopped_repeat_clock_tests.rs"]
mod stopped_repeat_clock_tests;

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "engine_stuck_note_tests.rs"]
mod stuck_note_tests;

#[cfg(test)]
#[path = "engine_note_edit_allocation_tests.rs"]
mod note_edit_allocation_tests;

#[cfg(test)]
#[path = "engine_mute_automation_tests.rs"]
mod mute_automation_tests;
#[cfg(test)]
#[path = "engine_mute_event_tests.rs"]
mod mute_event_tests;

#[cfg(test)]
#[path = "engine_note_repeat_tests.rs"]
mod note_repeat_tests;

#[cfg(test)]
#[path = "engine_clip_groove_tests.rs"]
mod clip_groove_tests;

#[cfg(test)]
#[path = "engine_section_playback_tests.rs"]
mod section_playback_tests;

#[cfg(test)]
#[path = "engine_section_queue_tests.rs"]
mod section_queue_tests;

#[cfg(test)]
#[path = "engine_section_record_tests.rs"]
mod section_record_tests;

#[cfg(test)]
#[path = "engine_capture_tests.rs"]
mod capture_tests;

#[path = "engine_clip_launcher.rs"]
mod clip_launcher;

#[cfg(test)]
#[path = "engine_clip_launcher_tests.rs"]
mod clip_launcher_tests;

#[path = "engine_graph_layout.rs"]
mod graph_layout;

#[path = "engine_processing.rs"]
mod processing;

#[path = "engine_graph.rs"]
mod graph_render;

#[cfg(test)]
#[path = "engine_compensation_tests.rs"]
mod compensation_tests;

#[cfg(test)]
#[path = "engine_compensation_clip_tests.rs"]
mod compensation_clip_tests;
#[cfg(test)]
#[path = "engine_graph_tests.rs"]
mod graph_tests;

#[path = "engine_device_handoff.rs"]
mod device_handoff;

#[path = "engine_reconfiguration_edits.rs"]
mod reconfiguration_edits;

#[path = "engine_reconfiguration.rs"]
pub mod reconfiguration;

#[path = "engine_device_observation.rs"]
mod device_observation;

#[path = "engine_device_validation.rs"]
mod device_validation;

#[path = "engine_handoff_observation.rs"]
mod handoff_observation;

#[path = "engine_capture_lifecycle.rs"]
mod capture_lifecycle;

#[path = "engine_capture_commands.rs"]
mod capture_commands;

#[path = "engine_capture_admission.rs"]
mod capture_admission;

#[path = "engine_compensation_commands.rs"]
mod compensation_commands;

#[path = "engine_compensation_plan.rs"]
mod compensation_plan;

#[path = "engine_device_commands.rs"]
mod device_commands;
#[path = "engine_instrument_commands.rs"]
mod instrument_commands;

#[path = "engine_effect_commands.rs"]
mod effect_commands;

#[cfg(test)]
#[path = "engine_bus_solo_tests.rs"]
mod bus_solo_tests;

#[path = "engine_offline_routing.rs"]
mod offline_routing;
pub(crate) use offline_routing::OfflineRoutingSetup;

#[path = "engine_offline_observation.rs"]
mod offline_observation;

#[cfg(test)]
#[path = "engine_compensation_stop_tests.rs"]
mod compensation_stop_tests;

#[cfg(test)]
#[path = "engine_offline_configuration_tests.rs"]
mod offline_configuration_tests;

#[cfg(test)]
#[path = "engine_reconfiguration_owner_tests.rs"]
mod reconfiguration_owner_tests;

#[cfg(test)]
#[path = "engine_recovery_configuration_tests.rs"]
mod recovery_configuration_tests;

#[cfg(test)]
#[path = "engine_graph_runtime_tests.rs"]
mod graph_runtime_tests;

#[cfg(test)]
#[path = "engine_presentation_runtime_tests.rs"]
mod presentation_runtime_tests;

#[cfg(test)]
#[path = "engine_time_domain_tests.rs"]
mod time_domain_tests;

#[cfg(test)]
#[path = "engine_command_allocation_tests.rs"]
mod command_allocation_tests;

#[cfg(test)]
#[path = "engine_clip_audio_retirement_tests.rs"]
mod clip_audio_retirement_tests;

#[cfg(test)]
#[path = "engine_compensation_presentation_tests.rs"]
mod compensation_presentation_tests;

#[path = "engine_compensation_transition.rs"]
mod compensation_transition;
#[path = "engine_output_gate.rs"]
mod output_gate;

#[cfg(test)]
#[path = "engine_compensation_transition_tests.rs"]
mod compensation_transition_tests;

#[cfg(test)]
#[path = "engine_compensation_edge_tests.rs"]
mod compensation_edge_tests;

#[cfg(test)]
#[path = "engine_recovery_review_tests.rs"]
mod recovery_review_tests;

#[path = "engine_transport_commands.rs"]
mod transport_commands;

#[path = "engine_presentation_queue.rs"]
mod presentation_queue;

#[path = "engine_terminal_notifications.rs"]
mod terminal_notifications;

#[path = "engine_presentation_state.rs"]
mod presentation_state;

#[path = "engine_clip_batch.rs"]
mod clip_batch;

#[cfg(test)]
#[path = "engine_section_owner_retention_tests.rs"]
mod section_owner_retention_tests;

#[cfg(test)]
#[path = "engine_presentation_review_tests.rs"]
mod presentation_review_tests;

#[cfg(test)]
#[path = "engine_recording_observation_tests.rs"]
mod recording_observation_tests;

#[cfg(test)]
#[path = "engine_transport_timing_review_tests.rs"]
mod transport_timing_review_tests;

#[cfg(test)]
#[path = "engine_automation_review_tests.rs"]
mod automation_review_tests;

#[cfg(test)]
#[path = "engine_automation_retirement_tests.rs"]
mod automation_retirement_tests;

#[cfg(test)]
#[path = "engine_device_observation_tests.rs"]
mod device_observation_tests;

#[cfg(test)]
#[path = "engine_device_observation_scan_tests.rs"]
mod device_observation_scan_tests;

#[cfg(test)]
#[path = "engine_device_observation_validation_tests.rs"]
mod device_observation_validation_tests;

#[cfg(test)]
#[path = "engine_device_observation_capture_tests.rs"]
mod device_observation_capture_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_admission_tests.rs"]
mod capture_lifecycle_admission_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_episode_tests.rs"]
mod capture_lifecycle_episode_tests;

#[cfg(test)]
#[path = "engine_capture_start_rearm_tests.rs"]
mod capture_start_rearm_tests;

#[cfg(test)]
#[path = "engine_capture_start_rearm_control_tests.rs"]
mod capture_start_rearm_control_tests;

#[cfg(test)]
#[path = "engine_capture_start_rearm_pressure_tests.rs"]
mod capture_start_rearm_pressure_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_recovery_tests.rs"]
mod capture_lifecycle_recovery_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_pressure_tests.rs"]
mod capture_lifecycle_pressure_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_transport_tests.rs"]
mod capture_lifecycle_transport_tests;

#[cfg(test)]
#[path = "engine_capture_lifecycle_rebase_tests.rs"]
mod capture_lifecycle_rebase_tests;
