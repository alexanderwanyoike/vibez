//! Arrange inner growth remains separate from live Section and launcher source boundaries.

use crate::playback_source::note_value_insertion_tests::{NOTE_ARRAY_ALLOCATIONS, INSERTION_CONTROL_ALLOCATIONS};

use super::*;
use crate::clip_insertion::{ClipInsertionOutcome, PreparedNoteInsertion};
use crate::events::EngineEvent;
use crate::playback_source::note_value_insertion_tests::ClipSnapshot;

#[test]
fn pending_note_value_growth_keeps_all_source_homes_and_perform_clocks_distinct() {
    for home in [
        PendingHome::Section,
        PendingHome::LauncherReplacement,
        PendingHome::LauncherStop,
    ] {
        let PendingHomeFixture {
            mut engine,
            mut commands,
            mut events,
            track_id,
            calls,
            arrange,
            inactive,
            initial_section,
            old_home,
            next_home,
            next_section,
        } = prepare_pending_home(home);
        // Section launch already returned and dropped its initial empty Box.
        // Its historical address may be reused by the later queued source.
        let homes = [
            ("Arrange", arrange.source),
            ("inactive launcher", inactive.source),
            ("active performance", old_home.source),
            ("queued performance", next_home.source),
        ];
        for (index, (name, source)) in homes.iter().enumerate() {
            for (other_name, other) in &homes[index + 1..] {
                assert_ne!(source, other, "simultaneously live {name} and {other_name}");
            }
            if !matches!(home, PendingHome::Section) {
                assert_ne!(
                    *source, initial_section.source,
                    "live initial Section and {name}"
                );
            }
        }
        if matches!(home, PendingHome::Section) {
            old_home.assert_unchanged(&engine.tracks()[0].section_playback_source);
        } else {
            initial_section.assert_unchanged(&engine.tracks()[0].section_playback_source);
        }
        let resident = &engine.tracks()[0].playback_source.note_clips[0];
        let original = ClipSnapshot::observe(resident);
        original.assert_full();
        let clip_id = resident.id;
        let note = MidiNote {
            pitch: 60,
            velocity: 93,
            start_beat: 1.0,
            duration_beats: 0.5,
        };
        let insertion = PreparedNoteInsertion::new(note);
        let operation = insertion.id();
        commands
            .push(EngineCommand::AddNote {
                track_id,
                clip_id,
                insertion,
            })
            .unwrap();
        commands
            .push(EngineCommand::SetTrackGain(track_id, 0.5))
            .unwrap();
        measured_callback(&mut engine, &mut []);
        let mut request = None;
        while let Ok(event) = events.pop() {
            if let EngineEvent::ClipInsertionStorageRequested(value) = event {
                assert_eq!(value.id, operation);
                assert_eq!(
                    (value.track_id, value.target),
                    (
                        track_id,
                        crate::clip_insertion::ClipInsertionTarget::Clip(clip_id)
                    )
                );
                assert!(request.replace(value).is_none());
            } else {
                assert!(!matches!(event, EngineEvent::ClipInsertionCompleted(_)));
            }
        }
        let request =
            request.expect("drain must address the restored Arrange note/cache allocation");
        let mut displaced = 0;
        let mut output = [0.0; 16];
        for step in 0..4 {
            measured_callback(&mut engine, &mut output);
            let expected = if matches!(home, PendingHome::LauncherStop) && step >= 1 {
                0.0
            } else {
                0.25
            };
            for &[left, right] in output.as_chunks::<2>().0 {
                assert_eq!((left, right), (expected, 0.0), "{home:?} block {step}");
            }
            arrange.assert_unchanged(&engine.tracks()[0].playback_source);
            original.assert_unchanged(&engine.tracks()[0].playback_source.note_clips[0]);
            assert_eq!(engine.tracks()[0].gain, 1.0);
            assert_eq!(
                commands.slots(),
                vibez_core::constants::RING_BUFFER_CAPACITY - 1
            );
            assert_eq!(engine.transport().position(), 0);
            let mut heard_perform = None;
            while let Ok(event) = events.pop() {
                match event {
                    EngineEvent::SectionTransitioned(crate::events::SectionTransitionedEvent {
                        section_id,
                        effective_at_samples,
                        retired,
                    }) => {
                        assert!(matches!(home, PendingHome::Section));
                        assert_eq!(section_id, next_section);
                        assert_eq!(effective_at_samples, 32);
                        old_home.assert_unchanged(&retired.tracks()[0].source);
                        displaced += 1;
                        drop(retired);
                    }
                    EngineEvent::ClipTransitioned {
                        request_id,
                        effective_at_samples,
                        retired,
                        ..
                    } => {
                        assert!(!matches!(home, PendingHome::Section));
                        assert_eq!(request_id, 2);
                        assert_eq!(effective_at_samples, 16);
                        old_home.assert_unchanged(&retired.unwrap().source);
                        displaced += 1;
                    }
                    EngineEvent::PerformancePosition(position) => heard_perform = Some(position),
                    EngineEvent::ClipInsertionStorageRequested(_)
                    | EngineEvent::ClipInsertionCompleted(_) => {
                        panic!(
                            "pending inner growth must not duplicate or complete without its reply"
                        );
                    }
                    _ => {}
                }
            }
            assert_eq!(heard_perform, Some(16 + step * 8), "{home:?}");
        }
        assert_eq!(displaced, 1);
        let channel = &engine.tracks()[0];
        inactive.assert_unchanged(&channel.empty_launcher_source);
        if matches!(home, PendingHome::Section) {
            next_home.assert_unchanged(&channel.section_playback_source);
        } else {
            next_home.assert_unchanged(&channel.launcher_source);
            initial_section.assert_unchanged(&channel.section_playback_source);
        }
        let before = calls.observed();
        let length = engine.transport().audio_length();
        request.fulfill();
        measured_callback(&mut engine, &mut []);
        let EngineEvent::ClipInsertionCompleted(owner) = events.pop().unwrap() else {
            panic!("Arrange note growth must complete ahead of its FIFO gain witness");
        };
        assert_eq!(owner.id(), operation);
        assert_eq!(owner.outcome, ClipInsertionOutcome::Applied);
        assert_eq!(
            (owner.track_id, owner.target),
            (
                track_id,
                crate::clip_insertion::ClipInsertionTarget::Clip(clip_id)
            )
        );
        assert_eq!(
            crate::retirement::tests::allocations(|| drop(owner)),
            (0, NOTE_ARRAY_ALLOCATIONS * 2 + INSERTION_CONTROL_ALLOCATIONS)
        );
        assert_eq!(
            commands.slots(),
            vibez_core::constants::RING_BUFFER_CAPACITY
        );
        assert_eq!(engine.transport().audio_length(), length);
        let channel = &engine.tracks()[0];
        assert_eq!(channel.gain, 0.5);
        assert_eq!(
            std::ptr::from_ref(channel.playback_source.as_ref()),
            arrange.source
        );
        assert_eq!(channel.playback_source.note_clips.len(), 1);
        assert_eq!(channel.playback_source.note_clips.as_ptr(), arrange.live);
        assert_eq!(
            channel.playback_source.note_clips.capacity(),
            arrange.live_capacity
        );
        assert_eq!(
            channel.playback_source.removed_note_clips.as_ptr(),
            arrange.staging
        );
        assert_eq!(
            channel.playback_source.removed_note_clips.capacity(),
            arrange.staging_capacity
        );
        assert!(channel.playback_source.removed_note_clips.is_empty());
        original.assert_appended(&channel.playback_source.note_clips[0], note);
        inactive.assert_unchanged(&channel.empty_launcher_source);
        if matches!(home, PendingHome::Section) {
            next_home.assert_unchanged(&channel.section_playback_source);
        } else {
            next_home.assert_unchanged(&channel.launcher_source);
            initial_section.assert_unchanged(&channel.section_playback_source);
        }
        assert_eq!(
            calls.observed(),
            before,
            "Arrange insertion cannot touch a performance voice"
        );
        measured_callback(&mut engine, &mut output);
        let expected = if matches!(home, PendingHome::LauncherStop) {
            0.0
        } else {
            0.125
        };
        for &[left, right] in output.as_chunks::<2>().0 {
            assert_eq!((left, right), (expected, 0.0));
        }
        assert_eq!(calls.observed(), before);
    }
}
