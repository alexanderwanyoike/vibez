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

#[derive(Debug)]
struct OnsetScore {
    correct: usize,
    false_positives: usize,
    false_negatives: usize,
    doubled: usize,
    signed_errors: Vec<i64>,
}

impl OnsetScore {
    fn precision(&self) -> f32 {
        self.correct as f32 / (self.correct + self.false_positives).max(1) as f32
    }

    fn recall(&self) -> f32 {
        self.correct as f32 / (self.correct + self.false_negatives).max(1) as f32
    }

    fn f1(&self) -> f32 {
        let precision = self.precision();
        let recall = self.recall();
        if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        }
    }
}

fn score_onsets(detected: &[u64], expected: &[u64], tolerance: u64) -> OnsetScore {
    let mut matched = vec![false; expected.len()];
    let mut signed_errors = Vec::new();
    let mut false_positives = 0;
    let mut doubled = 0;

    for &candidate in detected {
        let best = expected
            .iter()
            .enumerate()
            .filter(|(index, onset)| !matched[*index] && candidate.abs_diff(**onset) <= tolerance)
            .min_by_key(|(_, onset)| candidate.abs_diff(**onset));
        if let Some((index, onset)) = best {
            matched[index] = true;
            signed_errors.push(candidate as i64 - *onset as i64);
        } else {
            false_positives += 1;
            if expected
                .iter()
                .any(|onset| candidate.abs_diff(*onset) <= tolerance)
            {
                doubled += 1;
            }
        }
    }

    let correct = matched.iter().filter(|matched| **matched).count();
    OnsetScore {
        correct,
        false_positives,
        false_negatives: expected.len() - correct,
        doubled,
        signed_errors,
    }
}

#[test]
fn beating_decay_tails_are_not_detected_as_new_attacks() {
    let sample_rate = 44_100u32;
    let (audio, expected) = ringing_attack_train(sample_rate);
    let detected = detect_onsets(&audio, TransientSensitivity::DEFAULT);
    let score = score_onsets(
        &detected,
        &expected,
        frames_for_ms(sample_rate, 25.0) as u64,
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
    assert_eq!(score.signed_errors.len(), expected.len());
    let five_ms = frames_for_ms(sample_rate, 5.0) as i64;
    assert!(
        score
            .signed_errors
            .iter()
            .all(|error| error.abs() <= five_ms),
        "slice boundaries exceed 5 ms: {score:?}"
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
            let score = score_onsets(
                &detected,
                &expected,
                frames_for_ms(sample_rate, 25.0) as u64,
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
    let score = score_onsets(
        &detected,
        &expected,
        frames_for_ms(sample_rate, 25.0) as u64,
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
    let backtracked = backtrack_events_to_minima(&events, &energy, hop, usize::MAX);

    assert_eq!(backtracked, [0, (2 * hop) as u64, (5 * hop) as u64]);
    for (event, boundary) in events.iter().zip(&backtracked) {
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
        backtrack_events_to_minima(&[event], &energy, hop, hop),
        [event]
    );
}
