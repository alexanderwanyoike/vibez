use vibez_core::id::{SectionId, TrackId};
use vibez_core::perform::NoteRepeatRate;

use crate::playback_source::PreparedSectionPlaybackSource;

/// Events sent from the audio engine back to the UI thread (via rtrb).
///
/// These are pushed by the engine inside the real-time audio callback and
/// consumed on the UI thread to update the interface.  Because they are
/// produced in the audio callback, every variant must be trivially cheap to
/// construct (no allocations, no locks).
/// Opaque, shareable holder carrying a boxed device across the event
/// ring for disposal. Exists so `EngineEvent` can keep deriving
/// Debug/Clone/PartialEq: clones share the same cell, equality is
/// cell identity, and whichever holder takes the box first drops it.
pub struct DisposalCell<T: ?Sized>(std::sync::Arc<std::sync::Mutex<Option<Box<T>>>>);

impl<T: ?Sized> DisposalCell<T> {
    pub fn new(device: Box<T>) -> Self {
        Self(std::sync::Arc::new(std::sync::Mutex::new(Some(device))))
    }

    /// Take the device out for dropping; None if already taken.
    pub fn take(&self) -> Option<Box<T>> {
        self.0.lock().ok().and_then(|mut guard| guard.take())
    }
}

impl<T: ?Sized> Clone for DisposalCell<T> {
    fn clone(&self) -> Self {
        Self(std::sync::Arc::clone(&self.0))
    }
}

impl<T: ?Sized> PartialEq for DisposalCell<T> {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T: ?Sized> std::fmt::Debug for DisposalCell<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DisposalCell")
    }
}

#[derive(Debug)]
pub enum EngineEvent {
    /// A device removed from the audio graph, handed back so the UI
    /// thread performs the teardown. Plugin destructors run dlclose
    /// and COM/JUCE teardown, which must never happen in the audio
    /// callback (RT-unsafe) or off the plugin's main thread (JUCE
    /// MessageManager affinity: deadlocks).
    DisposeEffect(DisposalCell<dyn vibez_dsp::effect::AudioEffect>),
    /// See [`EngineEvent::DisposeEffect`].
    DisposeInstrument(DisposalCell<dyn vibez_instruments::Instrument>),

    /// The canonical Arrange cursor expressed as an absolute sample offset.
    /// Perform playback deliberately leaves this value unchanged.
    PlaybackPosition(u64),

    /// Monotonic, zero-based time for the current Perform session.
    PerformancePosition(u64),

    /// Peak and RMS meter readings for the most recent audio buffer.
    Metering {
        peak_l: f32,
        peak_r: f32,
        rms_l: f32,
        rms_r: f32,
    },

    /// Per-track peak meter readings.
    TrackMeter {
        track_id: TrackId,
        peak_l: f32,
        peak_r: f32,
    },

    /// A track mute command became effective in the audio engine at this
    /// absolute transport sample. This is the authoritative event later
    /// Capture work consumes.
    TrackMuteChanged {
        track_id: TrackId,
        muted: bool,
        effective_at_samples: u64,
    },
    /// A manual control took precedence over automation, or automation
    /// was explicitly re-enabled.
    AutomationOverrideChanged {
        track_id: TrackId,
        target: vibez_core::automation::AutomationTarget,
        overridden: bool,
    },

    /// A generated Note Repeat retrigger became effective at this exact
    /// engine sample. Later recording cards consume the same audible truth.
    NoteRepeated {
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: NoteRepeatRate,
        effective_at_samples: u64,
        canonical_at_samples: u64,
        section_id: Option<SectionId>,
        section_position_samples: Option<u64>,
        canonical_section_position_samples: Option<u64>,
    },

    /// A monitored input note became effective on the engine clock. Section
    /// Record consumes these events; monitoring itself remains immediate.
    InstrumentNoteInput {
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        on: bool,
        effective_at_samples: u64,
        section_id: Option<SectionId>,
        section_position_samples: Option<u64>,
    },

    /// Section Record is waiting for an engine-owned musical boundary.
    SectionRecordArmed {
        section_id: SectionId,
        track_id: TrackId,
        effective_at_samples: u64,
        section_position_samples: u64,
    },

    /// Section Record crossed its boundary and now accepts timestamped input.
    SectionRecordStarted {
        section_id: SectionId,
        track_id: TrackId,
        effective_at_samples: u64,
        section_position_samples: u64,
    },

    /// Section Record stopped. A cancelled count-in returns its resident owner
    /// so it is dropped on the UI thread, never in the audio callback.
    SectionRecordStopped {
        section_id: SectionId,
        track_id: TrackId,
        effective_at_samples: u64,
        section_position_samples: u64,
        started: bool,
        retired: Option<Box<PreparedSectionPlaybackSource>>,
    },

    /// Capture into Arrange began on this exact engine boundary. An already
    /// active Section includes its exact local playhead for a mid-loop start.
    PerformanceCaptureStarted {
        effective_at_samples: u64,
        section_id: Option<SectionId>,
        section_position_samples: Option<u64>,
    },

    /// Capture into Arrange stopped on this exact engine boundary.
    PerformanceCaptureStopped { effective_at_samples: u64 },

    /// A resident Section is queued for this exact transport sample.
    /// Re-queueing returns the displaced resident owner for UI-thread drop.
    SectionQueued {
        section_id: SectionId,
        effective_at_samples: u64,
        retired: Option<Box<PreparedSectionPlaybackSource>>,
    },

    /// A queued resident owner was cancelled by transport stop.
    SectionQueueCancelled {
        retired: Box<PreparedSectionPlaybackSource>,
    },

    /// A resident Section became active at this exact transport sample.
    /// `retired` carries displaced sources to the UI thread for destruction.
    SectionTransitioned {
        section_id: SectionId,
        effective_at_samples: u64,
        retired: Box<PreparedSectionPlaybackSource>,
    },

    /// A Section source refresh was consumed by the engine. `applied` is
    /// false when that Section stopped or changed before the command arrived.
    /// In both cases `retired` returns ownership to the UI thread.
    SectionSourceRefreshed {
        section_id: SectionId,
        applied: bool,
        retired: Box<PreparedSectionPlaybackSource>,
    },

    /// Current engine-owned local Section playhead.
    SectionPlaybackPosition {
        section_id: SectionId,
        position_samples: u64,
    },

    /// Playback has started (transport entered playing state).
    PlaybackStarted,

    /// Playback has stopped (transport entered stopped state).
    PlaybackStopped,

    /// The post-master Audition Bus became silent after stop or source end.
    AuditionStopped,
    /// Audition is waiting for its transport beat/bar boundary.
    AuditionQueued,
    /// Audition crossed its requested boundary and began rendering.
    AuditionStarted,
}

impl PartialEq for EngineEvent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::DisposeEffect(left), Self::DisposeEffect(right)) => left == right,
            (Self::DisposeInstrument(left), Self::DisposeInstrument(right)) => left == right,
            (Self::PlaybackPosition(left), Self::PlaybackPosition(right)) => left == right,
            (Self::PerformancePosition(left), Self::PerformancePosition(right)) => left == right,
            (
                Self::Metering {
                    peak_l: left_peak_l,
                    peak_r: left_peak_r,
                    rms_l: left_rms_l,
                    rms_r: left_rms_r,
                },
                Self::Metering {
                    peak_l: right_peak_l,
                    peak_r: right_peak_r,
                    rms_l: right_rms_l,
                    rms_r: right_rms_r,
                },
            ) => {
                left_peak_l == right_peak_l
                    && left_peak_r == right_peak_r
                    && left_rms_l == right_rms_l
                    && left_rms_r == right_rms_r
            }
            (
                Self::TrackMeter {
                    track_id: left_track,
                    peak_l: left_peak_l,
                    peak_r: left_peak_r,
                },
                Self::TrackMeter {
                    track_id: right_track,
                    peak_l: right_peak_l,
                    peak_r: right_peak_r,
                },
            ) => {
                left_track == right_track
                    && left_peak_l == right_peak_l
                    && left_peak_r == right_peak_r
            }
            (
                Self::TrackMuteChanged {
                    track_id: left_track,
                    muted: left_muted,
                    effective_at_samples: left_effective,
                },
                Self::TrackMuteChanged {
                    track_id: right_track,
                    muted: right_muted,
                    effective_at_samples: right_effective,
                },
            ) => {
                left_track == right_track
                    && left_muted == right_muted
                    && left_effective == right_effective
            }
            (
                Self::AutomationOverrideChanged {
                    track_id: left_track,
                    target: left_target,
                    overridden: left_overridden,
                },
                Self::AutomationOverrideChanged {
                    track_id: right_track,
                    target: right_target,
                    overridden: right_overridden,
                },
            ) => {
                left_track == right_track
                    && left_target == right_target
                    && left_overridden == right_overridden
            }
            (
                Self::NoteRepeated {
                    track_id: left_track,
                    pitch: left_pitch,
                    velocity: left_velocity,
                    rate: left_rate,
                    effective_at_samples: left_effective,
                    canonical_at_samples: left_canonical,
                    section_id: left_section,
                    section_position_samples: left_position,
                    canonical_section_position_samples: left_canonical_position,
                },
                Self::NoteRepeated {
                    track_id: right_track,
                    pitch: right_pitch,
                    velocity: right_velocity,
                    rate: right_rate,
                    effective_at_samples: right_effective,
                    canonical_at_samples: right_canonical,
                    section_id: right_section,
                    section_position_samples: right_position,
                    canonical_section_position_samples: right_canonical_position,
                },
            ) => {
                left_track == right_track
                    && left_pitch == right_pitch
                    && left_velocity == right_velocity
                    && left_rate == right_rate
                    && left_effective == right_effective
                    && left_canonical == right_canonical
                    && left_section == right_section
                    && left_position == right_position
                    && left_canonical_position == right_canonical_position
            }
            (
                Self::InstrumentNoteInput {
                    track_id: lt,
                    pitch: lp,
                    velocity: lv,
                    on: lo,
                    effective_at_samples: le,
                    section_id: ls,
                    section_position_samples: lsp,
                },
                Self::InstrumentNoteInput {
                    track_id: rt,
                    pitch: rp,
                    velocity: rv,
                    on: ro,
                    effective_at_samples: re,
                    section_id: rs,
                    section_position_samples: rsp,
                },
            ) => lt == rt && lp == rp && lv == rv && lo == ro && le == re && ls == rs && lsp == rsp,
            (
                Self::SectionRecordArmed {
                    section_id: ls,
                    track_id: lt,
                    effective_at_samples: le,
                    section_position_samples: lp,
                },
                Self::SectionRecordArmed {
                    section_id: rs,
                    track_id: rt,
                    effective_at_samples: re,
                    section_position_samples: rp,
                },
            )
            | (
                Self::SectionRecordStarted {
                    section_id: ls,
                    track_id: lt,
                    effective_at_samples: le,
                    section_position_samples: lp,
                },
                Self::SectionRecordStarted {
                    section_id: rs,
                    track_id: rt,
                    effective_at_samples: re,
                    section_position_samples: rp,
                },
            ) => ls == rs && lt == rt && le == re && lp == rp,
            (
                Self::SectionRecordStopped {
                    section_id: ls,
                    track_id: lt,
                    effective_at_samples: le,
                    section_position_samples: lp,
                    started: lstarted,
                    retired: lr,
                },
                Self::SectionRecordStopped {
                    section_id: rs,
                    track_id: rt,
                    effective_at_samples: re,
                    section_position_samples: rp,
                    started: rstarted,
                    retired: rr,
                },
            ) => {
                ls == rs
                    && lt == rt
                    && le == re
                    && lp == rp
                    && lstarted == rstarted
                    && match (lr, rr) {
                        (Some(left), Some(right)) => std::ptr::eq(left.as_ref(), right.as_ref()),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (
                Self::PerformanceCaptureStarted {
                    effective_at_samples: le,
                    section_id: ls,
                    section_position_samples: lp,
                },
                Self::PerformanceCaptureStarted {
                    effective_at_samples: re,
                    section_id: rs,
                    section_position_samples: rp,
                },
            ) => le == re && ls == rs && lp == rp,
            (
                Self::PerformanceCaptureStopped {
                    effective_at_samples: left,
                },
                Self::PerformanceCaptureStopped {
                    effective_at_samples: right,
                },
            ) => left == right,
            (
                Self::SectionQueued {
                    section_id: left_section,
                    effective_at_samples: left_effective,
                    retired: left_retired,
                },
                Self::SectionQueued {
                    section_id: right_section,
                    effective_at_samples: right_effective,
                    retired: right_retired,
                },
            ) => {
                left_section == right_section
                    && left_effective == right_effective
                    && match (left_retired, right_retired) {
                        (Some(left), Some(right)) => std::ptr::eq(left.as_ref(), right.as_ref()),
                        (None, None) => true,
                        _ => false,
                    }
            }
            (
                Self::SectionQueueCancelled {
                    retired: left_retired,
                },
                Self::SectionQueueCancelled {
                    retired: right_retired,
                },
            ) => std::ptr::eq(left_retired.as_ref(), right_retired.as_ref()),
            (
                Self::SectionTransitioned {
                    section_id: left_section,
                    effective_at_samples: left_effective,
                    retired: left_retired,
                },
                Self::SectionTransitioned {
                    section_id: right_section,
                    effective_at_samples: right_effective,
                    retired: right_retired,
                },
            ) => {
                left_section == right_section
                    && left_effective == right_effective
                    && std::ptr::eq(left_retired.as_ref(), right_retired.as_ref())
            }
            (
                Self::SectionPlaybackPosition {
                    section_id: left_section,
                    position_samples: left_position,
                },
                Self::SectionPlaybackPosition {
                    section_id: right_section,
                    position_samples: right_position,
                },
            ) => left_section == right_section && left_position == right_position,
            (
                Self::SectionSourceRefreshed {
                    section_id: left_section,
                    applied: left_applied,
                    retired: left_retired,
                },
                Self::SectionSourceRefreshed {
                    section_id: right_section,
                    applied: right_applied,
                    retired: right_retired,
                },
            ) => {
                left_section == right_section
                    && left_applied == right_applied
                    && std::ptr::eq(left_retired.as_ref(), right_retired.as_ref())
            }
            (Self::PlaybackStarted, Self::PlaybackStarted)
            | (Self::PlaybackStopped, Self::PlaybackStopped)
            | (Self::AuditionStopped, Self::AuditionStopped)
            | (Self::AuditionQueued, Self::AuditionQueued)
            | (Self::AuditionStarted, Self::AuditionStarted) => true,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_variants_are_constructible() {
        let _pos = EngineEvent::PlaybackPosition(44_100);
        let _performance_pos = EngineEvent::PerformancePosition(22_050);
        let _meter = EngineEvent::Metering {
            peak_l: 0.8,
            peak_r: 0.75,
            rms_l: 0.5,
            rms_r: 0.45,
        };
        let _track_meter = EngineEvent::TrackMeter {
            track_id: TrackId::new(),
            peak_l: 0.5,
            peak_r: 0.4,
        };
        let _track_mute = EngineEvent::TrackMuteChanged {
            track_id: TrackId::new(),
            muted: true,
            effective_at_samples: 44_100,
        };
        let _automation_override = EngineEvent::AutomationOverrideChanged {
            track_id: TrackId::new(),
            target: vibez_core::automation::AutomationTarget::TrackMute,
            overridden: true,
        };
        let _started = EngineEvent::PlaybackStarted;
        let _stopped = EngineEvent::PlaybackStopped;
        let _audition_stopped = EngineEvent::AuditionStopped;
        let _audition_queued = EngineEvent::AuditionQueued;
        let _audition_started = EngineEvent::AuditionStarted;
    }

    #[test]
    fn events_can_be_sent_through_rtrb() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<EngineEvent>::new(16);

        producer.push(EngineEvent::PlaybackStarted).unwrap();
        producer.push(EngineEvent::PlaybackPosition(512)).unwrap();
        producer
            .push(EngineEvent::PerformancePosition(256))
            .unwrap();
        producer
            .push(EngineEvent::Metering {
                peak_l: 0.9,
                peak_r: 0.85,
                rms_l: 0.6,
                rms_r: 0.55,
            })
            .unwrap();
        producer.push(EngineEvent::PlaybackStopped).unwrap();

        assert!(matches!(
            consumer.pop().unwrap(),
            EngineEvent::PlaybackStarted
        ));
        assert!(matches!(
            consumer.pop().unwrap(),
            EngineEvent::PlaybackPosition(512)
        ));
        assert!(matches!(
            consumer.pop().unwrap(),
            EngineEvent::PerformancePosition(256)
        ));

        match consumer.pop().unwrap() {
            EngineEvent::Metering {
                peak_l,
                peak_r,
                rms_l,
                rms_r,
            } => {
                assert!((peak_l - 0.9).abs() < f32::EPSILON);
                assert!((peak_r - 0.85).abs() < f32::EPSILON);
                assert!((rms_l - 0.6).abs() < f32::EPSILON);
                assert!((rms_r - 0.55).abs() < f32::EPSILON);
            }
            other => panic!("expected Metering, got {other:?}"),
        }

        assert!(matches!(
            consumer.pop().unwrap(),
            EngineEvent::PlaybackStopped
        ));
    }

    #[test]
    fn track_meter_can_be_sent_through_rtrb() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<EngineEvent>::new(16);
        let tid = TrackId::new();

        producer
            .push(EngineEvent::TrackMeter {
                track_id: tid,
                peak_l: 0.7,
                peak_r: 0.6,
            })
            .unwrap();

        match consumer.pop().unwrap() {
            EngineEvent::TrackMeter {
                track_id,
                peak_l,
                peak_r,
            } => {
                assert_eq!(track_id, tid);
                assert!((peak_l - 0.7).abs() < f32::EPSILON);
                assert!((peak_r - 0.6).abs() < f32::EPSILON);
            }
            other => panic!("expected TrackMeter, got {other:?}"),
        }
    }

    #[test]
    fn event_debug() {
        let event = EngineEvent::Metering {
            peak_l: 1.0,
            peak_r: 0.0,
            rms_l: 0.5,
            rms_r: 0.5,
        };
        let debug = format!("{event:?}");
        assert!(debug.contains("Metering"));
    }
}
