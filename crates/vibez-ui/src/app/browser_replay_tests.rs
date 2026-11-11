//! Replaying the selected Browser sample from the keyboard.

use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::track::MediaSourceRef;
use vibez_engine::commands::EngineCommand;

use crate::message::Message;

fn app_with_loaded_selection() -> (super::App, rtrb::Consumer<EngineCommand>) {
    let mut app = super::test_support::app();
    let (commands, received) = rtrb::RingBuffer::new(16);
    app.cmd_tx = crate::domains::EngineCommandQueue::new(
        commands,
        vibez_engine::transport_ring::transport_ring().0,
    );
    let source = MediaSourceRef::LocalFile {
        path: "/samples/kick.wav".into(),
    };
    let browser = &mut app.state.browser;
    browser.open = true;
    browser.keyboard_focus = true;
    browser.selected_source = Some(source.clone());
    browser.waveform_source = Some(source);
    browser.waveform_audio = Some(Arc::new(DecodedAudio {
        channels: vec![vec![0.0; 64]],
        sample_rate: app.state.transport.sample_rate,
    }));
    (app, received)
}

fn started_auditions(received: &mut rtrb::Consumer<EngineCommand>) -> usize {
    std::iter::from_fn(|| received.pop().ok())
        .filter(|command| matches!(command, EngineCommand::StartAudition { .. }))
        .count()
}

#[test]
fn replaying_the_selected_sample_starts_its_audition_again() {
    let (mut app, mut received) = app_with_loaded_selection();

    let _ = app.update(Message::ReplaySelectedBrowserSample);
    let _ = app.update(Message::ReplaySelectedBrowserSample);

    assert_eq!(started_auditions(&mut received), 2);
}

#[test]
fn replay_stays_silent_while_audition_is_off() {
    let (mut app, mut received) = app_with_loaded_selection();
    app.state.browser.audition_enabled = false;

    let _ = app.update(Message::ReplaySelectedBrowserSample);

    assert_eq!(started_auditions(&mut received), 0);
}

#[test]
fn replay_stays_silent_after_the_producer_works_outside_the_browser() {
    let (mut app, mut received) = app_with_loaded_selection();
    app.state.browser.keyboard_focus = false;

    let _ = app.update(Message::ReplaySelectedBrowserSample);

    assert_eq!(started_auditions(&mut received), 0);
}

#[test]
fn replay_waits_until_the_selected_sample_has_loaded() {
    let (mut app, mut received) = app_with_loaded_selection();
    app.state.browser.waveform_audio = None;

    let _ = app.update(Message::ReplaySelectedBrowserSample);

    assert_eq!(started_auditions(&mut received), 0);
}

#[test]
fn replay_stays_silent_while_the_browser_is_closed() {
    let (mut app, mut received) = app_with_loaded_selection();
    app.state.browser.open = false;

    let _ = app.update(Message::ReplaySelectedBrowserSample);

    assert_eq!(started_auditions(&mut received), 0);
}
