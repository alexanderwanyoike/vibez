use super::*;

#[test]
fn stop_and_space_finalize_an_active_audio_recording() {
    assert_eq!(
        audio_recording_transport_guard(
            crate::domains::audio_recording::AudioRecordingPhase::Recording,
            &TransportMsg::Stop,
        ),
        AudioRecordingTransportGuard::StopRecording
    );
    assert_eq!(
        audio_recording_transport_guard(
            crate::domains::audio_recording::AudioRecordingPhase::Recording,
            &TransportMsg::TogglePlayback,
        ),
        AudioRecordingTransportGuard::StopRecording
    );
}

#[test]
fn recording_blocks_seek_loop_and_tempo_changes_before_the_transport_domain() {
    for message in [
        TransportMsg::Seek(0.5),
        TransportMsg::SeekToBeat(8.0),
        TransportMsg::ToggleArrangementLoop,
        TransportMsg::SetArrangementLoopRegion {
            start_beats: 4.0,
            end_beats: 8.0,
        },
        TransportMsg::BpmSubmit,
        TransportMsg::NudgeBpm(1.0),
    ] {
        assert_eq!(
            audio_recording_transport_guard(
                crate::domains::audio_recording::AudioRecordingPhase::Recording,
                &message,
            ),
            AudioRecordingTransportGuard::BlockTimelineChange
        );
    }
}

#[test]
fn the_internal_stop_passes_to_transport_after_recording_enters_stopping() {
    assert_eq!(
        audio_recording_transport_guard(
            crate::domains::audio_recording::AudioRecordingPhase::Stopping,
            &TransportMsg::Stop,
        ),
        AudioRecordingTransportGuard::Pass
    );
}
