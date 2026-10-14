//! Persisted effect identities select independent, repeatable source-frame dither streams.

use vibez_core::{effect::EffectType, id::EffectId};
use vibez_dsp::factory::create_effect_for_instance;

#[test]
fn persisted_effect_ids_render_their_frozen_streams_after_reconstruction_and_reset() {
    // Fixed independent tables prevent instance routing from sharing the production RNG oracle.
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
    let mut identity_outputs = Vec::new();
    for (stored_id, codes) in cases {
        let id: EffectId = serde_json::from_str(stored_id).unwrap();
        let serialized = serde_json::to_string(&id).unwrap();
        let restored: EffectId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(id, restored);
        let expected: Vec<_> = codes.into_iter().flatten().map(|code| f32::from(code) * 0.125).collect();
        let mut first_output = None;
        for parts in [&[32][..], &[1, 7, 3, 5, 16][..]] {
            let mut effect = create_effect_for_instance(
                EffectType::Bitcrush, 48_000.0, &[4.0, 1.0, 1.0, 0.0, 0.0, 1.0], restored);
            for repeat in 0..2 {
                let mut audio: Vec<_> = (0..32).flat_map(|_| [0.03125, 0.0]).collect();
                let mut position = 0;
                for &frames in parts {
                    let end = position + frames * 2;
                    effect.process(&mut audio[position..end], 2);
                    position = end;
                }
                assert_eq!(audio, expected,
                    "stored_id={stored_id}, parts={parts:?}, reset_repeat={repeat}");
                if first_output.is_none() { first_output = Some(audio.clone()); }
                effect.reset();
            }
        }
        identity_outputs.push(first_output.unwrap());
    }
    assert_ne!(identity_outputs[0], identity_outputs[1], "separate persisted instances must not share dither");
}
