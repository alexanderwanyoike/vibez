//! Evaluate Vibez transient detection against a private annotation manifest.
//!
//! Run with:
//! `cargo run -p vibez-audio-io --example transient_benchmark -- manifest.json`

use std::{env, fs, path::PathBuf, process};

use serde::Deserialize;
use vibez_audio_io::file_io::decode_audio_file;
use vibez_core::onset::{
    detect_onsets,
    metrics::{evaluate, Evaluation},
    TransientSensitivity,
};

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
