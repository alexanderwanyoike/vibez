//! Reference response bins retain real wet Delay samples and inclusive finite windows.

use super::delay_response::{render_reference, DelayResponse};
use std::sync::atomic::AtomicBool;
use vibez_core::{audio_context::DeviceAudioContext, delay_state::DelayState};
use vibez_dsp::{
    delay::{param, DelayEffect},
    effect::AudioEffect,
};

fn applied_state(times: [f32; 2], cuts: bool) -> DelayState {
    let mut effect = DelayEffect::new(1000.0);
    for (index, value) in [
        (param::TIME_MS, times[0]),
        (param::RIGHT_TIME_MS, times[1]),
        (param::LINK, 0.0),
        (param::ROUTING, 1.0),
        (param::FEEDBACK, 0.5),
        (param::MIX, 0.22),
        (param::DUCK, 1.0),
        (param::FEEDBACK_HP_ENABLED, f32::from(u8::from(cuts))),
        (param::FEEDBACK_HP_HZ, 100.0),
        (param::FEEDBACK_LP_ENABLED, f32::from(u8::from(cuts))),
        (param::FEEDBACK_LP_HZ, 20000.0),
    ] {
        assert!(effect.set_param(index, value));
    }
    effect.set_audio_context(DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: 1000,
        bpm: 137.0,
        playing: true,
    });
    effect.process(&mut [0.25, 0.125], 2);
    let state = effect.delay_snapshot().unwrap();
    assert!(!state.reference_moving);
    assert!(state.duck_attenuation_db > 0.0);
    state
}

fn assert_bins(response: &DelayResponse, expected: &[[f32; 2]]) {
    assert_eq!(response.sample_rate, 1000);
    assert_eq!(response.frames, expected.len() as u64);
    assert_eq!(response.bins.len(), 512);
    for (index, bin) in response.bins.iter().enumerate() {
        let start = index * expected.len() / 512;
        let end = (index + 1) * expected.len() / 512;
        let mut peaks = [0.0_f32; 2];
        let mut frames = [if start < end { start as u32 } else { u32::MAX }; 2];
        for (frame, pair) in expected.iter().enumerate().take(end).skip(start) {
            for channel in 0..2 {
                if pair[channel].abs() > peaks[channel] {
                    peaks[channel] = pair[channel].abs();
                    frames[channel] = frame as u32;
                }
            }
        }
        assert_eq!(
            [bin.left.to_bits(), bin.right.to_bits()],
            peaks.map(f32::to_bits)
        );
        assert_eq!([bin.left_frame, bin.right_frame], frames, "bin {index}");
    }
}

#[test]
fn wet_reference_uses_real_kernel_inclusive_window_and_exact_peak_bins() {
    let cancel = AtomicBool::new(false);
    let state = applied_state([3.0, 5.0], false);
    let response = render_reference(&state, &cancel).expect("settled applied state is renderable");
    let mut expected = [[0.0; 2]; 21];
    for (frame, channel, amplitude) in [
        (3, 0, 1.0),
        (8, 1, 0.5),
        (11, 0, 0.25),
        (16, 1, 0.125),
        (19, 0, 0.0625),
    ] {
        expected[frame][channel] = amplitude;
    }
    assert_bins(&response, &expected);

    // A nonzero fourth echo at the window boundary detects an omitted endpoint.
    let equal = applied_state([5.0, 5.0], false);
    let endpoint = render_reference(&equal, &cancel).unwrap();
    let mut expected_endpoint = [[0.0; 2]; 21];
    for (frame, channel, amplitude) in [(5, 0, 1.0), (10, 1, 0.5), (15, 0, 0.25), (20, 1, 0.125)] {
        expected_endpoint[frame][channel] = amplitude;
    }
    assert_bins(&endpoint, &expected_endpoint);

    let filtered = applied_state([123.0, 175.5], true);
    let rendered = render_reference(&filtered, &cancel).unwrap();
    let mut direct = DelayEffect::new(filtered.sample_rate as f32);
    for (index, value) in filtered.targets.into_iter().enumerate() {
        let value = if index == param::MIX {
            1.0
        } else if index == param::DUCK {
            0.0
        } else {
            value
        };
        assert!(direct.set_param(index, value));
    }
    direct.set_audio_context(DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: filtered.sample_rate,
        bpm: filtered.bpm,
        playing: false,
    });
    direct.reset();
    let mut audio = [0.0_f32; 1406];
    audio[0] = 1.0;
    direct.process(&mut audio, 2);
    assert_bins(&rendered, audio.as_chunks::<2>().0);
    assert!(audio.iter().any(|sample| *sample != 0.0));

    assert!(render_reference(&state, &AtomicBool::new(true)).is_none());
    let mut moving = state;
    moving.reference_moving = true;
    assert!(render_reference(&moving, &cancel).is_none());
}
