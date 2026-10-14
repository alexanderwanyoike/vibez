//! Gate behavior must depend on its first two channels without corrupting wider buses.

use vibez_dsp::{effect::AudioEffect, gate::GateEffect};

#[test]
fn four_channel_gate_preserves_extra_channels_and_matches_stereo_processing() {
    for rate in [44_100.0, 48_000.0, 96_000.0] {
        let mut wider = Vec::new();
        let mut stereo = Vec::new();
        for frame in 0..4096 {
            let left = if (1024..2048).contains(&frame) { 0.5 } else { 0.001 };
            let right = -left * 0.6;
            wider.extend_from_slice(&[left, right, 0.9, -0.8]);
            stereo.extend_from_slice(&[left, right]);
        }
        let mut gate = GateEffect::new(rate);
        let mut reference = GateEffect::new(rate);
        reference.process(&mut stereo, 2);
        gate.process(&mut wider, 4);
        for (frame, (actual, expected)) in wider.chunks_exact(4).zip(stereo.chunks_exact(2)).enumerate() {
            assert_eq!(actual[2], 0.9, "extra channel2 changed at{rate}Hz/frame{frame}");
            assert_eq!(actual[3], -0.8, "extra channel3 changed at{rate}Hz/frame{frame}");
            assert_eq!(&actual[..2], expected,
                "first-two detection or gain changed at{rate}Hz/frame{frame}");
        }
    }
}
