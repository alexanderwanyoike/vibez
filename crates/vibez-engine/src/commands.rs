use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;

/// Commands sent from the UI thread to the audio engine (via rtrb).
///
/// These are pushed into an `rtrb::Producer<EngineCommand>` on the UI side
/// and drained from an `rtrb::Consumer<EngineCommand>` inside the real-time
/// audio callback.  Every variant must be safe to construct on the UI thread
/// and safe to drop on the audio thread without blocking.
pub enum EngineCommand {
    /// Start playback from the current transport position.
    Play,
    /// Stop playback (transport position is preserved).
    Stop,
    /// Seek the transport to an absolute sample position.
    Seek(u64),
    /// Change the project tempo.
    SetBpm(f64),
    /// Load decoded audio into the engine for playback.
    LoadAudio(Arc<DecodedAudio>),
    /// Remove any loaded audio from the engine.
    UnloadAudio,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_variants_are_constructible() {
        let _play = EngineCommand::Play;
        let _stop = EngineCommand::Stop;
        let _seek = EngineCommand::Seek(44_100);
        let _bpm = EngineCommand::SetBpm(140.0);

        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 100], vec![0.0; 100]],
            sample_rate: 44_100,
        });
        let _load = EngineCommand::LoadAudio(audio);
        let _unload = EngineCommand::UnloadAudio;
    }

    #[test]
    fn command_can_be_sent_through_rtrb() {
        let (mut producer, mut consumer) = rtrb::RingBuffer::<EngineCommand>::new(16);

        producer.push(EngineCommand::Play).unwrap();
        producer.push(EngineCommand::Seek(1000)).unwrap();
        producer.push(EngineCommand::SetBpm(90.0)).unwrap();
        producer.push(EngineCommand::Stop).unwrap();

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Play));

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Seek(1000)));

        let cmd = consumer.pop().unwrap();
        match cmd {
            EngineCommand::SetBpm(bpm) => assert!((bpm - 90.0).abs() < f64::EPSILON),
            _ => panic!("expected SetBpm"),
        }

        let cmd = consumer.pop().unwrap();
        assert!(matches!(cmd, EngineCommand::Stop));
    }

    #[test]
    fn load_audio_shares_arc() {
        let audio = Arc::new(DecodedAudio {
            channels: vec![vec![1.0, 2.0]],
            sample_rate: 48_000,
        });
        let cmd = EngineCommand::LoadAudio(Arc::clone(&audio));

        // The Arc should have 2 strong references now.
        assert_eq!(Arc::strong_count(&audio), 2);

        match cmd {
            EngineCommand::LoadAudio(a) => {
                assert_eq!(a.num_frames(), 2);
                assert_eq!(a.sample_rate, 48_000);
            }
            _ => panic!("expected LoadAudio"),
        }
    }
}
