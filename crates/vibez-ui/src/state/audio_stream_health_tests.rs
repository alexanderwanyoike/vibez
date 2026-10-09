use super::{AppState, AudioStreamHealth};
use vibez_audio_io::audio_stream::AudioStreamEvent;

#[test]
fn stream_error_and_recovery_update_persistent_health_and_status() {
    let mut state = AppState {
        audio_cpu_load_percent: 45.0,
        ..AppState::default()
    };

    state.apply_audio_stream_event(AudioStreamEvent::Error(
        "device disconnected mid-session".into(),
    ));
    assert_eq!(
        state.audio_stream_health,
        AudioStreamHealth::Error("device disconnected mid-session".into())
    );
    assert_eq!(
        state.status_text,
        "Audio stream error: device disconnected mid-session"
    );
    assert_eq!(state.audio_cpu_load_percent, 0.0);

    state.apply_audio_stream_event(AudioStreamEvent::Rebuilding);
    assert_eq!(state.audio_stream_health, AudioStreamHealth::Rebuilding);
    assert_eq!(state.status_text, "Rebuilding audio stream…");

    state.apply_audio_stream_event(AudioStreamEvent::Recovered);
    assert_eq!(state.audio_stream_health, AudioStreamHealth::Running);
    assert_eq!(state.status_text, "Audio stream recovered");

    state.apply_audio_stream_event(AudioStreamEvent::Running);
    assert_eq!(state.audio_stream_health, AudioStreamHealth::Running);
    assert_eq!(state.status_text, "Audio stream recovered");
}

#[test]
fn rejected_configuration_keeps_the_working_stream_healthy() {
    let mut state = AppState::default();

    state.apply_audio_stream_event(AudioStreamEvent::Rebuilding);
    assert_eq!(state.audio_stream_health, AudioStreamHealth::Rebuilding);

    state.apply_audio_stream_event(AudioStreamEvent::ConfigurationRejected(
        "unsupported rate".into(),
    ));

    assert_eq!(state.audio_stream_health, AudioStreamHealth::Running);
    assert_eq!(
        state.status_text,
        "Audio configuration rejected — previous output remains active: unsupported rate"
    );
}
