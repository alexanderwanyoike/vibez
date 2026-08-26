//! Evaluate Vibez transient detection against a private annotation manifest.
//!
//! Run with:
//! `cargo run -p vibez-audio-io --example transient_benchmark -- manifest.json`

use std::{env, fs, path::PathBuf, process};

use serde::Deserialize;
use vibez_audio_io::file_io::decode_audio_file;
use vibez_core::onset::{detect_onsets, TransientSensitivity};

#[derive(Debug, Deserialize)]
struct Manifest {
    tolerance_ms: f64,
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    name: String,
    category: String,
    path: PathBuf,
    /// Producer-approved source frames. The implicit clip-start boundary is
    /// excluded because Vibez does not display it as a Transient Marker.
    expected_frames: Vec<u64>,
    #[serde(default = "default_sensitivity")]
    sensitivity_percent: u8,
}

fn default_sensitivity() -> u8 {
    TransientSensitivity::DEFAULT.percent()
}

#[derive(Debug, Default)]
struct Evaluation {
    correct: usize,
    false_positives: usize,
    false_negatives: usize,
    doubled: usize,
    signed_errors_ms: Vec<f64>,
}

impl Evaluation {
    fn precision(&self) -> f64 {
        self.correct as f64 / (self.correct + self.false_positives).max(1) as f64
    }

    fn recall(&self) -> f64 {
        self.correct as f64 / (self.correct + self.false_negatives).max(1) as f64
    }

    fn f1(&self) -> f64 {
        let precision = self.precision();
        let recall = self.recall();
        if precision + recall == 0.0 {
            0.0
        } else {
            2.0 * precision * recall / (precision + recall)
        }
    }

    fn merge(&mut self, other: Self) {
        self.correct += other.correct;
        self.false_positives += other.false_positives;
        self.false_negatives += other.false_negatives;
        self.doubled += other.doubled;
        self.signed_errors_ms.extend(other.signed_errors_ms);
    }

    fn median_absolute_error_ms(&self) -> f64 {
        percentile(
            self.signed_errors_ms
                .iter()
                .map(|error| error.abs())
                .collect(),
            0.5,
        )
    }

    fn p95_absolute_error_ms(&self) -> f64 {
        percentile(
            self.signed_errors_ms
                .iter()
                .map(|error| error.abs())
                .collect(),
            0.95,
        )
    }

    fn mean_signed_error_ms(&self) -> f64 {
        if self.signed_errors_ms.is_empty() {
            0.0
        } else {
            self.signed_errors_ms.iter().sum::<f64>() / self.signed_errors_ms.len() as f64
        }
    }
}

fn percentile(mut values: Vec<f64>, percentile: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f64::total_cmp);
    let index = ((values.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len() - 1);
    values[index]
}

fn evaluate(
    detected: &[u64],
    expected: &[u64],
    tolerance_frames: u64,
    sample_rate: u32,
) -> Evaluation {
    let mut matched = vec![false; expected.len()];
    let mut evaluation = Evaluation::default();

    for &candidate in detected {
        let nearest = expected
            .iter()
            .enumerate()
            .filter(|(index, onset)| {
                !matched[*index] && candidate.abs_diff(**onset) <= tolerance_frames
            })
            .min_by_key(|(_, onset)| candidate.abs_diff(**onset));
        if let Some((index, onset)) = nearest {
            matched[index] = true;
            evaluation.correct += 1;
            evaluation
                .signed_errors_ms
                .push((candidate as f64 - *onset as f64) * 1_000.0 / f64::from(sample_rate));
        } else {
            evaluation.false_positives += 1;
            if expected
                .iter()
                .any(|onset| candidate.abs_diff(*onset) <= tolerance_frames)
            {
                evaluation.doubled += 1;
            }
        }
    }
    evaluation.false_negatives = matched.iter().filter(|matched| !**matched).count();
    evaluation
}

fn report(label: &str, evaluation: &Evaluation) {
    println!(
        "{label}: P={:.3} R={:.3} F1={:.3} FP={} FN={} doubled={} mean_signed={:.2}ms median_abs={:.2}ms p95_abs={:.2}ms",
        evaluation.precision(),
        evaluation.recall(),
        evaluation.f1(),
        evaluation.false_positives,
        evaluation.false_negatives,
        evaluation.doubled,
        evaluation.mean_signed_error_ms(),
        evaluation.median_absolute_error_ms(),
        evaluation.p95_absolute_error_ms(),
    );
}

fn run() -> Result<(), String> {
    let manifest_path = env::args()
        .nth(1)
        .ok_or_else(|| "usage: transient_benchmark <manifest.json>".to_owned())?;
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("read {manifest_path}: {error}"))?;
    let manifest: Manifest = serde_json::from_str(&manifest_text)
        .map_err(|error| format!("parse {manifest_path}: {error}"))?;
    let mut total = Evaluation::default();

    for case in manifest.cases {
        let audio = decode_audio_file(&case.path)
            .map_err(|error| format!("decode {}: {error}", case.path.display()))?;
        let detected = detect_onsets(&audio, TransientSensitivity::new(case.sensitivity_percent));
        let tolerance_frames =
            (manifest.tolerance_ms * f64::from(audio.sample_rate) / 1_000.0).round() as u64;
        let evaluation = evaluate(
            &detected,
            &case.expected_frames,
            tolerance_frames,
            audio.sample_rate,
        );
        report(&format!("{} [{}]", case.name, case.category), &evaluation);
        total.merge(evaluation);
    }

    report("TOTAL", &total);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_to_one_matching_counts_a_second_detection_as_doubled() {
        let evaluation = evaluate(&[100, 105, 200], &[100, 200], 10, 1_000);
        assert_eq!(evaluation.correct, 2);
        assert_eq!(evaluation.false_positives, 1);
        assert_eq!(evaluation.false_negatives, 0);
        assert_eq!(evaluation.doubled, 1);
        assert_eq!(evaluation.f1(), 0.8);
    }
}
