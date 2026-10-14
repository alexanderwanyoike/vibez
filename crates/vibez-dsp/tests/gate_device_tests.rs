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

#[test]
fn enabled_range_applies_a_closed_floor_while_disabled_range_preserves_legacy_zero() {
    let rate = 48_000.0;
    let make_gate = |depth: f32, enabled: bool| {
        let mut gate = GateEffect::new(rate);
        assert!(gate.set_param(0, -20.0));
        assert!(gate.set_param(1, 0.1));
        assert!(gate.set_param(2, 5.0));
        assert!(gate.set_param(3, 0.0));
        assert!(gate.set_param(6, depth), "the final Range index6 must be supported");
        assert!(gate.set_param(5, if enabled { 1.0 } else { 0.0 }),
            "the final Range Enabled index5 must be supported");
        gate
    };
    let mut disabled = make_gate(12.0, false);
    let mut dry_closed = vec![0.01; 4800];
    disabled.process(&mut dry_closed, 1);
    assert!(dry_closed.iter().all(|sample| *sample == 0.0),
        "disabled Range changed the original zero closed target");
    for depth in [0.0_f32, 12.0, 60.0] {
        let mut enabled = make_gate(depth, true);
        let mut signal = vec![0.01; 4800];
        enabled.process(&mut signal, 1);
        let expected = 0.01 * 10.0_f32.powf(-depth / 20.0);
        let tolerance = 2e-8 + expected * 2e-6;
        for sample in &signal[signal.len() - 128..] {
            assert!((*sample - expected).abs() <= tolerance,
                "Range {depth}dB settled at {sample}, expected {expected}");
        }
    }
}
