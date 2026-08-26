//! Offline clip DSP tasks: quantize, BPM and transient detection, transpose, auto-warp.

use super::super::audio_tasks::{compute_audio_quantize, AutoWarpInput, QuantizeInput};
use super::run_off_ui_thread;
use std::sync::Arc;

pub(in crate::app) async fn quantize_audio_clip_async(
    input: QuantizeInput,
) -> Result<crate::message::AudioQuantizeSuccess, String> {
    tokio::task::spawn_blocking(move || compute_audio_quantize(input))
        .await
        .map_err(|e| format!("quantize task failed: {e}"))?
}

pub(in crate::app) async fn detect_clip_bpm_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sample_rate: u32,
) -> Option<vibez_core::onset::BpmEstimate> {
    tokio::task::spawn_blocking(move || vibez_core::onset::detect_bpm(&audio, sample_rate))
        .await
        .unwrap_or(None)
}

pub(in crate::app) async fn detect_clip_transients_async(
    audio: Arc<vibez_core::audio_buffer::DecodedAudio>,
    sensitivity: vibez_core::onset::TransientSensitivity,
) -> Vec<u64> {
    tokio::task::spawn_blocking(move || vibez_core::onset::detect_onsets(&audio, sensitivity))
        .await
        .unwrap_or_default()
}

pub(in crate::app) async fn transpose_clip_async(
    request: crate::domains::arrangement::ClipTransposeRenderRequest,
) -> Result<crate::message::ClipTransposeSuccess, String> {
    run_off_ui_thread("clip transpose", move || {
        let (audio, transpose_fallback) =
            vibez_dsp::time_stretch::pitch_preserving_stretch_transposed_checked(
                &request.source_audio,
                request.target_frames,
                f32::from(request.transpose.semitones()),
            );
        crate::message::ClipTransposeSuccess {
            audio: Arc::new(audio),
            source_audio: request.source_audio,
            transpose: request.transpose,
            expected_warped: request.expected_warped,
            expected_audio: request.expected_audio,
            expected_geometry: request.expected_geometry,
            geometry: request.geometry,
            warning: transpose_fallback.then(|| {
                "Signalsmith declined this render; timing was restored but Transpose was not applied"
                    .to_string()
            }),
        }
    })
    .await
}

pub(in crate::app) async fn auto_warp_clip_async(
    input: AutoWarpInput,
) -> crate::message::AutoWarpOutcome {
    use crate::message::AutoWarpOutcome;
    let audio_for_detect = Arc::clone(&input.audio);
    let sample_rate = input.sample_rate;
    let estimate = tokio::task::spawn_blocking(move || {
        vibez_core::onset::detect_bpm(&audio_for_detect, sample_rate)
    })
    .await
    .unwrap_or(None);
    let Some(est) = estimate else {
        return AutoWarpOutcome::NotDetected;
    };
    if est.confidence < input.confidence_threshold || est.bpm <= 0.0 {
        return AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        };
    }
    let num_frames = input.audio.num_frames();
    let warp_input = crate::warp::WarpClipInput {
        audio: input.audio,
        fields_frames: num_frames as u64,
        source_offset: 0,
        start_marker: 0,
        duration: num_frames as u64,
        loop_start: 0,
        loop_end: 0,
        clip_bpm: est.bpm,
        project_bpm: input.project_bpm,
        transpose_semitones: 0,
    };
    match crate::warp::warp_clip_async(warp_input).await {
        Ok(success) => AutoWarpOutcome::Warped {
            confidence: est.confidence,
            success,
        },
        Err(_) => AutoWarpOutcome::DetectedOnly {
            bpm: est.bpm,
            confidence: est.confidence,
        },
    }
}
