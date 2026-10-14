//! Verifies the real UI prepared-owner queue retains persisted TPDF streams and forwarded rate semantics.

use super::{EngineCommandQueue, EngineHandle};
use vibez_core::{effect::EffectType, id::{EffectId, TrackId}};
use vibez_engine::commands::EngineCommand;

#[test]
fn ui_queue_prepares_persisted_tpdf_owners_at_forwarded_rates_and_restarts_their_streams() {
    // Frozen independent code tables keep the production factory/RNG out of the oracle.
    let cases: [(&str, [[i8; 2]; 32]); 2] = [
        ("17", [
            [1, 0],
            [0, 0],
            [1, 0],
            [-1, 0],
            [0, 0],
            [1, 0],
            [0, 0],
            [0, 0],
            [0, 0],
            [0, 0],
            [0, 0],
            [1, 0],
            [0, 0],
            [0, 1],
            [0, 0],
            [0, 0],
            [-1, 1],
            [0, 0],
            [0, 1],
            [0, 0],
            [1, 0],
            [0, 0],
            [1, 0],
            [0, 0],
            [1, 0],
            [0, 0],
            [0, 1],
            [0, 1],
            [0, 0],
            [1, 1],
            [0, 0],
            [1, 0],
        ]),
        ("9999", [
            [0, 0],
            [0, 0],
            [1, -1],
            [0, -1],
            [1, 0],
            [1, 1],
            [0, 0],
            [0, 1],
            [1, -1],
            [0, 0],
            [0, 0],
            [0, 0],
            [1, -1],
            [0, 0],
            [1, 0],
            [0, 0],
            [0, 0],
            [1, 0],
            [0, 1],
            [1, -1],
            [0, 0],
            [1, 0],
            [0, 0],
            [0, 0],
            [1, 1],
            [0, 0],
            [0, 0],
            [0, 0],
            [0, 0],
            [1, 0],
            [1, 1],
            [1, 0],
        ]),
    ];
    let params = [4.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 12_000.0];
    assert_eq!(params.len(), vibez_dsp::bitcrush::param::COUNT);
    let track: TrackId = serde_json::from_str("421").unwrap();
    let mut identity_outputs = Vec::new();
    for (stored_id, codes) in cases {
        let id: EffectId = serde_json::from_str(stored_id).unwrap();
        let saved = serde_json::to_string(&id).unwrap();
        let expected: Vec<_> = codes.into_iter().flatten()
            .map(|code| f32::from(code) * 0.125).collect();
        let mut first_output = None;
        for sample_rate in [44_100, 48_000, 96_000] {
            for parts in [&[32][..], &[1, 7, 3, 5, 16][..]] {
                for reconstruction in 0..2 {
                    let restored: EffectId = serde_json::from_str(&saved).unwrap();
                    assert_eq!(restored, id);
                    let (producer, mut consumer) = rtrb::RingBuffer::new(1);
                    let mut queue = EngineCommandQueue::new(producer);
                    queue.send(EngineCommand::SetSampleRate(sample_rate));
                    queue.insert_built_in_effect_with_params(track, restored, EffectType::Bitcrush, &params);
                    assert_eq!(queue.pending_len(), 1, "prepared insertion waits behind the actual rate command");
                    assert_eq!(queue.prepared_effect_sample_rate(track, restored), Some(sample_rate));
                    assert!(matches!(consumer.pop(), Ok(EngineCommand::SetSampleRate(actual)) if actual == sample_rate));
                    assert!(consumer.pop().is_err());
                    queue.flush();
                    let Ok(EngineCommand::InsertEffect {
                        track_id, effect_id, mut effect, position,
                    }) = consumer.pop() else {
                        panic!("expected the UI EngineHandle's complete prepared effect");
                    };
                    assert_eq!((track_id, effect_id, position), (track, restored, None));
                    assert_eq!(effect.effect_type(), EffectType::Bitcrush);
                    assert_eq!(effect.activation_sample_rate(), Some(sample_rate));
                    assert_eq!(effect.latency_samples(), 0);
                    for (index, expected) in params.iter().enumerate() {
                        assert_eq!(effect.get_param(index), *expected, "complete saved sound parameter{index}");
                    }
                    assert_eq!(queue.pending_len(), 0);
                    assert!(consumer.pop().is_err());
                    let queued_rate = if sample_rate == 96_000 { 48_000 } else { 96_000 };
                    queue.send(EngineCommand::SetSampleRate(queued_rate));
                    assert_eq!(queue.prepared_effect_sample_rate(track, restored), None,
                        "a later requested clock cannot relabel an already prepared owner");
                    assert_eq!(effect.activation_sample_rate(), Some(sample_rate));
                    assert!(matches!(consumer.pop(), Ok(EngineCommand::SetSampleRate(actual)) if actual == queued_rate));

                    for reset_repeat in 0..2 {
                        let mut audio: Vec<_> = (0..32).flat_map(|_| [0.03125, 0.0]).collect();
                        let mut offset = 0;
                        for &frames in parts {
                            let end = offset + frames * 2;
                            effect.process(&mut audio[offset..end], 2);
                            offset = end;
                        }
                        assert_eq!(offset, audio.len());
                        assert_eq!(audio, expected,
                            "saved ID={stored_id}, Fs={sample_rate}, parts={parts:?}, reconstruction={reconstruction}, reset={reset_repeat}");
                        if first_output.is_none() { first_output = Some(audio.clone()); }
                        effect.reset();
                    }
                }
            }
        }
        identity_outputs.push(first_output.unwrap());
    }
    assert_ne!(identity_outputs[0], identity_outputs[1], "persisted instances keep independent streams");
}
