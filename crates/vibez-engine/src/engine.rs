use std::sync::Arc;

use rtrb::{Consumer, Producer, RingBuffer};
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::constants::RING_BUFFER_CAPACITY;

use crate::commands::EngineCommand;
use crate::events::EngineEvent;
use crate::metering;
use crate::transport::Transport;

/// The real-time audio engine.
///
/// `AudioEngine` lives on the audio thread.  Its [`process()`](AudioEngine::process)
/// method is called once per audio callback to fill an output buffer with audio
/// and communicate with the UI thread via lock-free ring buffers.
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
    transport: Transport,
    audio: Option<Arc<DecodedAudio>>,
    cmd_rx: Consumer<EngineCommand>,
    event_tx: Producer<EngineEvent>,
}

impl AudioEngine {
    /// Create a new audio engine.
    ///
    /// Returns `(engine, command_producer, event_consumer)`.  The caller
    /// should move `engine` to the audio thread and keep the producer /
    /// consumer on the UI thread.
    pub fn new() -> (Self, Producer<EngineCommand>, Consumer<EngineEvent>) {
        let (cmd_tx, cmd_rx) = RingBuffer::<EngineCommand>::new(RING_BUFFER_CAPACITY);
        let (event_tx, event_rx) = RingBuffer::<EngineEvent>::new(RING_BUFFER_CAPACITY);

        let engine = Self {
            transport: Transport::new(),
            audio: None,
            cmd_rx,
            event_tx,
        };

        (engine, cmd_tx, event_rx)
    }

    /// Process one audio callback worth of data.
    ///
    /// `output` is an interleaved stereo buffer (`[L0, R0, L1, R1, ...]`)
    /// that must be filled with audio.  `channels` is the number of
    /// interleaved channels (typically 2).
    ///
    /// This method is **lock-free and allocation-free**.  It:
    /// 1. Drains all pending commands from the ring buffer.
    /// 2. If playing and audio is loaded, copies samples into `output`.
    /// 3. Advances the transport.
    /// 4. Sends metering and position events to the UI thread.
    pub fn process(&mut self, output: &mut [f32], channels: usize) {
        // ---- 1. Drain commands ------------------------------------------
        self.drain_commands();

        // ---- 2. Fill output buffer --------------------------------------
        let frames = if channels > 0 {
            output.len() / channels
        } else {
            0
        };

        if self.transport.is_playing() {
            if let Some(ref audio) = self.audio {
                let pos = self.transport.position();
                let audio_channels = audio.num_channels();

                for frame in 0..frames {
                    let sample_idx = pos as usize + frame;

                    for ch in 0..channels {
                        let sample = if ch < audio_channels {
                            audio.sample(ch, sample_idx)
                        } else if audio_channels > 0 {
                            // If the output has more channels than the audio,
                            // duplicate the last available channel.
                            audio.sample(audio_channels - 1, sample_idx)
                        } else {
                            0.0
                        };
                        output[frame * channels + ch] = sample;
                    }
                }
            } else {
                // Playing but no audio loaded -- output silence.
                output.iter_mut().for_each(|s| *s = 0.0);
            }
        } else {
            // Stopped -- output silence.
            output.iter_mut().for_each(|s| *s = 0.0);
        }

        // ---- 3. Advance transport ---------------------------------------
        let new_pos = self.transport.advance(frames as u64);

        // ---- 4. Send events to the UI thread ----------------------------
        // Position event.
        let _ = self.event_tx.push(EngineEvent::PlaybackPosition(new_pos));

        // Metering event.
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

    /// Read the currently loaded audio (for inspection / testing).
    pub fn audio(&self) -> Option<&Arc<DecodedAudio>> {
        self.audio.as_ref()
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// Drain all pending commands from the ring buffer without blocking.
    fn drain_commands(&mut self) {
        while let Ok(cmd) = self.cmd_rx.pop() {
            match cmd {
                EngineCommand::Play => {
                    self.transport.play();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStarted);
                }
                EngineCommand::Stop => {
                    self.transport.stop();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                }
                EngineCommand::Seek(pos) => {
                    self.transport.seek(pos);
                }
                EngineCommand::SetBpm(bpm) => {
                    self.transport.set_bpm(bpm);
                }
                EngineCommand::LoadAudio(audio) => {
                    let len = audio.num_frames() as u64;
                    self.audio = Some(audio);
                    self.transport.set_audio_length(Some(len));
                }
                EngineCommand::UnloadAudio => {
                    self.audio = None;
                    self.transport.set_audio_length(None);
                    self.transport.stop();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::audio_buffer::DecodedAudio;

    /// Helper to create a simple stereo decoded audio with a known pattern.
    fn make_test_audio(frames: usize) -> Arc<DecodedAudio> {
        let left: Vec<f32> = (0..frames).map(|i| (i as f32) / (frames as f32)).collect();
        let right: Vec<f32> = (0..frames)
            .map(|i| -((i as f32) / (frames as f32)))
            .collect();
        Arc::new(DecodedAudio {
            channels: vec![left, right],
            sample_rate: 44_100,
        })
    }

    #[test]
    fn new_returns_ring_buffer_endpoints() {
        let (engine, _cmd_tx, _event_rx) = AudioEngine::new();
        assert!(!engine.transport().is_playing());
        assert!(engine.audio().is_none());
    }

    #[test]
    fn process_outputs_silence_when_stopped() {
        let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
        let mut buf = vec![999.0f32; 512];
        engine.process(&mut buf, 2);

        assert!(buf.iter().all(|&s| s == 0.0));
    }

    #[test]
    fn play_command_starts_transport() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!(engine.transport().is_playing());

        // Should have received PlaybackStarted event.
        let mut found_started = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStarted {
                found_started = true;
            }
        }
        assert!(found_started);
    }

    #[test]
    fn stop_command_stops_transport() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();

        cmd_tx.push(EngineCommand::Play).unwrap();
        cmd_tx.push(EngineCommand::Stop).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!(!engine.transport().is_playing());

        let mut found_stopped = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStopped {
                found_stopped = true;
            }
        }
        assert!(found_stopped);
    }

    #[test]
    fn load_audio_and_play() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx
            .push(EngineCommand::LoadAudio(audio.clone()))
            .unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames stereo
        engine.process(&mut buf, 2);

        // The output should contain the first 8 frames of the test audio.
        for frame in 0..8 {
            let expected_l = audio.sample(0, frame);
            let expected_r = audio.sample(1, frame);
            let actual_l = buf[frame * 2];
            let actual_r = buf[frame * 2 + 1];
            assert!(
                (actual_l - expected_l).abs() < 1e-6,
                "frame {} L: expected {} got {}",
                frame,
                expected_l,
                actual_l
            );
            assert!(
                (actual_r - expected_r).abs() < 1e-6,
                "frame {} R: expected {} got {}",
                frame,
                expected_r,
                actual_r
            );
        }

        // Transport should have advanced by 8 frames.
        assert_eq!(engine.transport().position(), 8);
    }

    #[test]
    fn seek_then_play() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx
            .push(EngineCommand::LoadAudio(audio.clone()))
            .unwrap();
        cmd_tx.push(EngineCommand::Seek(100)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 8]; // 4 frames stereo
        engine.process(&mut buf, 2);

        // Should be playing from position 100.
        let expected_l = audio.sample(0, 100);
        assert!((buf[0] - expected_l).abs() < 1e-6);
        assert_eq!(engine.transport().position(), 104);
    }

    #[test]
    fn unload_audio_stops_and_clears() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);
        assert!(engine.audio().is_some());

        cmd_tx.push(EngineCommand::UnloadAudio).unwrap();

        let mut buf = vec![0.0f32; 16];
        engine.process(&mut buf, 2);

        assert!(engine.audio().is_none());
        assert!(!engine.transport().is_playing());

        // Drain events and check for PlaybackStopped.
        let mut found_stopped = false;
        while let Ok(event) = event_rx.pop() {
            if event == EngineEvent::PlaybackStopped {
                found_stopped = true;
            }
        }
        assert!(found_stopped);
    }

    #[test]
    fn set_bpm_command() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        cmd_tx.push(EngineCommand::SetBpm(140.0)).unwrap();

        let mut buf = vec![0.0f32; 8];
        engine.process(&mut buf, 2);

        assert!((engine.transport().bpm() - 140.0).abs() < f64::EPSILON);
    }

    #[test]
    fn metering_events_are_sent() {
        let (mut engine, mut cmd_tx, mut event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 512];
        engine.process(&mut buf, 2);

        let mut found_metering = false;
        while let Ok(event) = event_rx.pop() {
            if let EngineEvent::Metering { .. } = event {
                found_metering = true;
            }
        }
        assert!(found_metering);
    }

    #[test]
    fn position_events_are_sent() {
        let (mut engine, _cmd_tx, mut event_rx) = AudioEngine::new();

        let mut buf = vec![0.0f32; 64];
        engine.process(&mut buf, 2);

        let mut found_position = false;
        while let Ok(event) = event_rx.pop() {
            if let EngineEvent::PlaybackPosition(pos) = event {
                found_position = true;
                assert_eq!(pos, 0); // transport is stopped, position stays 0
            }
        }
        assert!(found_position);
    }

    #[test]
    fn auto_stop_at_end_of_audio() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(16); // only 16 frames

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        // Request 32 frames (more than the 16 available).
        let mut buf = vec![0.0f32; 64]; // 32 frames stereo
        engine.process(&mut buf, 2);

        // Transport should have auto-stopped at frame 16.
        assert!(!engine.transport().is_playing());
        assert_eq!(engine.transport().position(), 16);

        // Samples beyond the audio length should be 0 (DecodedAudio::sample
        // returns 0 for out-of-bounds).
        // Frames 16..31 should be silence.
        for frame in 16..32 {
            assert_eq!(buf[frame * 2], 0.0, "frame {} L should be 0", frame);
            assert_eq!(buf[frame * 2 + 1], 0.0, "frame {} R should be 0", frame);
        }
    }

    #[test]
    fn multiple_process_calls_advance_position() {
        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        let audio = make_test_audio(1024);

        cmd_tx.push(EngineCommand::LoadAudio(audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 128]; // 64 frames
        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 64);

        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 128);

        engine.process(&mut buf, 2);
        assert_eq!(engine.transport().position(), 192);
    }

    #[test]
    fn mono_audio_to_stereo_output() {
        let mono_audio = Arc::new(DecodedAudio {
            channels: vec![vec![0.5; 64]],
            sample_rate: 44_100,
        });

        let (mut engine, mut cmd_tx, _event_rx) = AudioEngine::new();
        cmd_tx.push(EngineCommand::LoadAudio(mono_audio)).unwrap();
        cmd_tx.push(EngineCommand::Play).unwrap();

        let mut buf = vec![0.0f32; 16]; // 8 frames stereo
        engine.process(&mut buf, 2);

        // Both channels should get the mono signal.
        for frame in 0..8 {
            assert!((buf[frame * 2] - 0.5).abs() < 1e-6);
            assert!((buf[frame * 2 + 1] - 0.5).abs() < 1e-6);
        }
    }

    #[test]
    fn process_with_zero_length_buffer() {
        let (mut engine, _cmd_tx, _event_rx) = AudioEngine::new();
        let mut buf: Vec<f32> = vec![];
        // Should not panic.
        engine.process(&mut buf, 2);
    }
}
