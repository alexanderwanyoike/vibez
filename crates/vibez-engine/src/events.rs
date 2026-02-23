/// Events sent from the audio engine back to the UI thread (via rtrb).
///
/// These are pushed by the engine inside the real-time audio callback and
/// consumed on the UI thread to update the interface.  Because they are
/// produced in the audio callback, every variant must be trivially cheap to
/// construct (no allocations, no locks).
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The current playback position expressed as an absolute sample offset.
    PlaybackPosition(u64),

    /// Peak and RMS meter readings for the most recent audio buffer.
    Metering {
        peak_l: f32,
        peak_r: f32,
        rms_l: f32,
        rms_r: f32,
    },

    /// Playback has started (transport entered playing state).
    PlaybackStarted,

    /// Playback has stopped (transport entered stopped state).
    PlaybackStopped,
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
        let _started = EngineEvent::PlaybackStarted;
        let _stopped = EngineEvent::PlaybackStopped;
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
            other => panic!("expected Metering, got {:?}", other),
        }

        assert_eq!(consumer.pop().unwrap(), EngineEvent::PlaybackStopped);
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

        let debug = format!("{:?}", event);
        assert!(debug.contains("Metering"));
    }
}
