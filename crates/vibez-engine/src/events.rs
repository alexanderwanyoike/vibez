use vibez_core::id::TrackId;

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

#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// A device removed from the audio graph, handed back so the UI
    /// thread performs the teardown. Plugin destructors run dlclose
    /// and COM/JUCE teardown, which must never happen in the audio
    /// callback (RT-unsafe) or off the plugin's main thread (JUCE
    /// MessageManager affinity: deadlocks).
    DisposeEffect(DisposalCell<dyn vibez_dsp::effect::AudioEffect>),
    /// See [`EngineEvent::DisposeEffect`].
    DisposeInstrument(DisposalCell<dyn vibez_instruments::Instrument>),

    /// The current playback position expressed as an absolute sample offset.
    PlaybackPosition(u64),

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_variants_are_constructible() {
        let _pos = EngineEvent::PlaybackPosition(44_100);
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
            .push(EngineEvent::Metering {
                peak_l: 0.9,
                peak_r: 0.85,
                rms_l: 0.6,
                rms_r: 0.55,
            })
            .unwrap();
        producer.push(EngineEvent::PlaybackStopped).unwrap();

        assert_eq!(consumer.pop().unwrap(), EngineEvent::PlaybackStarted);
        assert_eq!(consumer.pop().unwrap(), EngineEvent::PlaybackPosition(512));

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

        assert_eq!(consumer.pop().unwrap(), EngineEvent::PlaybackStopped);
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
    fn event_debug_and_clone() {
        let event = EngineEvent::Metering {
            peak_l: 1.0,
            peak_r: 0.0,
            rms_l: 0.5,
            rms_r: 0.5,
        };
        let cloned = event.clone();
        assert_eq!(event, cloned);

        let debug = format!("{event:?}");
        assert!(debug.contains("Metering"));
    }
}
