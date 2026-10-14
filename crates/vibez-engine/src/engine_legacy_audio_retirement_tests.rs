//! Legacy buffer edits preserve playback while returning unique audio owners off callback.

use super::*;

fn stereo_ramp(frames: usize, left: f32, right: f32, sample_rate: u32) -> Arc<DecodedAudio> {
    Arc::new(DecodedAudio {
        channels: vec![
            (0..frames).map(|frame| left + frame as f32 / 1024.0).collect(),
            (0..frames).map(|frame| right + frame as f32 / 2048.0).collect(),
        ],
        sample_rate,
    })
}

#[test]
fn replacing_unique_legacy_audio_preserves_playing_cursor_and_returns_old_owner_off_callback() {
    let (mut engine, mut commands, mut events) = AudioEngine::new();
    let old = stereo_ramp(256, 0.125, -0.25, 1000);
    let old_owner = Arc::downgrade(&old);
    commands.push(EngineCommand::SetSampleRate(1000)).unwrap();
    commands.push(EngineCommand::LoadAudio(old)).unwrap();
    commands.push(EngineCommand::Seek(17)).unwrap();
    commands.push(EngineCommand::Play).unwrap();
    let mut output = [0.0; 16];
    engine.process_block(AudioProcessBlock::new(&mut output, 2));
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (frame + 17) as f32;
        assert_eq!(actual, [0.125 + position / 1024.0, -0.25 + position / 2048.0]);
    }
    assert_eq!(engine.transport().position(), 25);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(256));
    assert_eq!(old_owner.strong_count(), 1, "resident legacy buffer is uniquely owned");
    while events.pop().is_ok() {}

    let replacement = stereo_ramp(512, 0.5, -0.75, 2000);
    let replacement_pointer = Arc::as_ptr(&replacement);
    commands.push(EngineCommand::LoadAudio(replacement)).unwrap();
    let measured = crate::retirement::tests::allocations(|| {
        engine.process_block(AudioProcessBlock::new(&mut output, 2));
    });
    for (frame, actual) in output.chunks_exact(2).enumerate() {
        let position = (frame + 25) as f32;
        assert_eq!(actual, [0.5 + position / 1024.0, -0.75 + position / 2048.0]);
    }
    assert_eq!(engine.audio().map(Arc::as_ptr), Some(replacement_pointer));
    assert_eq!(engine.transport().position(), 33);
    assert!(engine.transport().is_playing());
    assert_eq!(engine.transport().audio_length(), Some(512));
    assert_eq!(engine.sample_rate(), 1000, "loading media does not adopt its sample rate");
    assert_eq!(measured, (0, 0), "replacement cannot destroy its displaced unique owner");
    assert_eq!(old_owner.strong_count(), 1, "old owner survives until ordinary UI receipt");
    while events.pop().is_ok() {}
    assert_eq!(old_owner.strong_count(), 0, "owner-thread event consumption releases old audio");
}
