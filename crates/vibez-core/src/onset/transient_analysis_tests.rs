use super::metrics::evaluate;
use super::*;

fn make_audio(channels: Vec<Vec<f32>>, sample_rate: u32) -> DecodedAudio {
    DecodedAudio {
        channels,
        sample_rate,
    }
}

fn ringing_attack_train(sample_rate: u32) -> (DecodedAudio, Vec<u64>) {
    let starts =
        [2_000usize, 13_000, 24_000, 35_000].map(|frame| frame * sample_rate as usize / 44_100);
    let tail_frames = 7_000 * sample_rate as usize / 44_100;
    let decay_frames = 2_500.0 * sample_rate as f32 / 44_100.0;
    let mut channel = vec![0.0f32; sample_rate as usize];
    for start in starts {
        for offset in 0..tail_frames {
            if start + offset >= channel.len() {
                break;
            }
            let time = offset as f32 / sample_rate as f32;
            let decay = (-(offset as f32) / decay_frames).exp();
            channel[start + offset] += decay
                * ((std::f32::consts::TAU * 120.0 * time).sin()
                    + (std::f32::consts::TAU * 128.0 * time).sin())
                * 0.4;
        }
    }
    let expected = starts.into_iter().map(|frame| frame as u64).collect();
    (
        make_audio(vec![channel.clone(), channel], sample_rate),
        expected,
    )
}

#[test]
fn beating_decay_tails_are_not_detected_as_new_attacks() {
    let sample_rate = 44_100u32;
    let (audio, expected) = ringing_attack_train(sample_rate);
    let detected = detect_onsets(&audio, TransientSensitivity::DEFAULT);
    let score = evaluate(
        &detected,
        &expected,
        frames_for_ms(sample_rate, 25.0) as u64,
        sample_rate,
    );

    assert_eq!(
        score.false_positives, 0,
        "score={score:?}, onsets={detected:?}"
    );
    assert_eq!(
        score.false_negatives, 0,
        "score={score:?}, onsets={detected:?}"
    );
    assert_eq!(score.doubled, 0, "score={score:?}, onsets={detected:?}");
    assert_eq!(score.f1(), 1.0, "score={score:?}, onsets={detected:?}");
    assert_eq!(score.signed_errors_ms.len(), expected.len());
    assert!(
        score
            .signed_errors_ms
            .iter()
            .all(|error| error.abs() <= 7.0),
        "ringing-tail boundaries exceed aubio's 7 ms tolerance: {score:?}"
    );
}

#[test]
fn sharp_click_boundaries_land_within_one_localisation_hop() {
    let sample_rate = 44_100u32;
    let expected = [4_000u64, 14_000, 24_000, 34_000];
    let mut channel = vec![0.0f32; sample_rate as usize];
    for start in expected {
        let start = start as usize;
        for offset in 0..1_024 {
            let phase = std::f32::consts::TAU * 1_800.0 * offset as f32 / sample_rate as f32;
            channel[start + offset] = phase.cos() * (-(offset as f32) / 100.0).exp();
        }
    }
    let audio = make_audio(vec![channel.clone(), channel], sample_rate);
    let detected = detect_onsets(&audio, TransientSensitivity::DEFAULT);
    let score = evaluate(
        &detected,
        &expected,
        frames_for_ms(sample_rate, 10.0) as u64,
        sample_rate,
    );

    assert_eq!(score.f1(), 1.0, "score={score:?}, onsets={detected:?}");
    assert!(
        score
            .signed_errors_ms
            .iter()
            .all(|error| error.abs() <= 2.90),
        "sharp-click boundary exceeds one localisation hop: {score:?}"
    );
}

#[test]
fn transient_detection_is_sample_rate_and_gain_robust() {
    for sample_rate in [44_100u32, 48_000, 96_000] {
        let (mut audio, expected) = ringing_attack_train(sample_rate);
        for gain in [0.05f32, 0.5, 1.0] {
            for channel in &mut audio.channels {
                for sample in channel {
                    *sample *= gain;
                }
            }
            let detected = detect_onsets(&audio, TransientSensitivity::DEFAULT);
            let score = evaluate(
                &detected,
                &expected,
                frames_for_ms(sample_rate, 25.0) as u64,
                sample_rate,
            );
            assert_eq!(
                score.f1(),
                1.0,
                "sample_rate={sample_rate}, gain={gain}, score={score:?}, onsets={detected:?}"
            );

            for channel in &mut audio.channels {
                for sample in channel {
                    *sample /= gain;
                }
            }
        }
    }
}

#[test]
fn opposite_polarity_stereo_does_not_cancel_transient_analysis() {
    let sample_rate = 44_100u32;
    let (mut audio, expected) = ringing_attack_train(sample_rate);
    audio.channels[1]
        .iter_mut()
        .for_each(|sample| *sample = -*sample);

    let detected = detect_onsets(&audio, TransientSensitivity::DEFAULT);
    let score = evaluate(
        &detected,
        &expected,
        frames_for_ms(sample_rate, 25.0) as u64,
        sample_rate,
    );
    assert_eq!(score.f1(), 1.0, "score={score:?}, onsets={detected:?}");
}

// Ports the invariants asserted by librosa 0.11.0's test_onset_backtrack for
// both onset-envelope and RMS inputs.
#[test]
fn ported_librosa_backtracking_lands_on_a_preceding_energy_minimum() {
    let energy = [3.0, 2.0, 2.0, 4.0, 3.0, 1.0, 2.0];
    let hop = 128usize;
    let events = [hop as u64, (3 * hop) as u64, (6 * hop) as u64];
    let events = events.map(|event| (event, event));
    let backtracked = localize::backtrack_events_to_minima(&events, &energy, hop, usize::MAX);

    assert_eq!(backtracked, [0, (2 * hop) as u64, (5 * hop) as u64]);
    for ((event, _), boundary) in events.iter().zip(&backtracked) {
        assert!(*boundary <= *event, "backtracking moved an event later");
        let frame = *boundary as usize / hop;
        assert!(
            frame == 0 || energy[frame] <= energy[frame - 1],
            "boundary {boundary} did not land on a local minimum"
        );
    }
}

#[test]
fn backtracking_respects_the_daw_lookback_bound() {
    let energy = [3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 5.0];
    let hop = 128usize;
    let event = (6 * hop) as u64;
    assert_eq!(
        localize::backtrack_events_to_minima(&[(event, event)], &energy, hop, hop),
        [event]
    );
}
