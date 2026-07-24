use std::collections::HashSet;
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::{MidiNote, TrackKind};

use super::*;
use crate::domains::perform::{Section, SectionTimelineEditor};
use crate::domains::test_support::RecordingEngine;
use crate::state::{
    new_master_track, ClipClipboard, ProjectTrack, ProjectTracksState, TimelineEditorState, UiClip,
    UiNoteClip,
};

fn project_tracks(kinds: &[TrackKind]) -> ProjectTracksState {
    ProjectTracksState {
        tracks: kinds
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                let id = TrackId::new();
                if kind.is_midi() {
                    ProjectTrack::new_instrument(
                        id,
                        format!("MIDI {}", index + 1),
                        *kind,
                        index as u8,
                    )
                } else {
                    ProjectTrack::new(id, format!("Audio {}", index + 1), index as u8)
                }
            })
            .collect(),
        master: new_master_track(),
        buses: Vec::new(),
        next_track_number: kinds.len() as u32 + 1,
    }
}

fn audio_clip(position: u64) -> UiClip {
    UiClip {
        id: ClipId::new(),
        name: "Audio".into(),
        audio: Arc::new(DecodedAudio {
            channels: vec![vec![0.0; 800]],
            sample_rate: 48_000,
        }),
        source: None,
        position,
        source_offset: 0,
        duration: 200,
        loop_enabled: false,
        loop_start: 0,
        loop_end: 0,
        original_bpm: None,
        warped: false,
        warped_to_bpm: None,
        original_audio: None,
    }
}

fn note_clip(position_beats: f64) -> UiNoteClip {
    UiNoteClip {
        id: ClipId::new(),
        name: "MIDI".into(),
        position_beats,
        duration_beats: 2.0,
        notes: vec![MidiNote {
            pitch: 60,
            velocity: 100,
            start_beat: 0.25,
            duration_beats: 0.5,
        }],
        selected_notes: HashSet::from([0]),
        loop_enabled: false,
        loop_start_beats: 0.0,
        loop_end_beats: 0.0,
        groove_grid: vibez_core::perform::GrooveGrid::Off,
    }
}

fn copy(
    editor: &mut TimelineEditorState,
    project: &ProjectTracksState,
    clipboard: &mut ClipClipboard,
    ctx: ArrangementCtx,
) -> ArrangementAction {
    editor.update_clipboard(
        project,
        ArrangementMsg::CopySelectedClips,
        clipboard,
        &mut RecordingEngine::default(),
        ctx,
    )
}

fn paste(
    editor: &mut TimelineEditorState,
    project: &ProjectTracksState,
    clipboard: &mut ClipClipboard,
    ctx: ArrangementCtx,
) -> (ArrangementAction, RecordingEngine) {
    let mut engine = RecordingEngine::default();
    let action = editor.update_clipboard(
        project,
        ArrangementMsg::PasteClips,
        clipboard,
        &mut engine,
        ctx,
    );
    (action, engine)
}

#[test]
fn audio_cross_timeline_paste_remints_identity_shares_media_and_keeps_position() {
    let project = project_tracks(&[TrackKind::Audio, TrackKind::Audio]);
    let source_track = project.tracks[0].id;
    let destination_track = project.tracks[1].id;
    let source = audio_clip(600);
    let source_id = source.id;
    let media = Arc::clone(&source.audio);
    let mut source_editor = TimelineEditorState::default();
    Arc::make_mut(&mut source_editor.timeline)
        .ensure(source_track)
        .clips
        .push(source);
    source_editor
        .selected_clips
        .insert(ArrangementSelection::AudioClip {
            track_id: source_track,
            clip_id: source_id,
        });
    let mut clipboard = ClipClipboard::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        playhead_samples: 9_000,
        playhead_beats: 90.0,
    };

    copy(&mut source_editor, &project, &mut clipboard, ctx);
    let mut destination_editor = TimelineEditorState {
        selected_track: Some(destination_track),
        ..TimelineEditorState::default()
    };
    let (action, _) = paste(&mut destination_editor, &project, &mut clipboard, ctx);

    assert!(action.mark_dirty);
    let pasted = &destination_editor
        .timeline
        .get(destination_track)
        .unwrap()
        .clips[0];
    assert_ne!(pasted.id, source_id);
    assert_eq!(pasted.position, 600);
    assert!(Arc::ptr_eq(&pasted.audio, &media));
    assert_eq!(
        source_editor.timeline.get(source_track).unwrap().clips[0].id,
        source_id
    );
}

#[test]
fn midi_cross_timeline_paste_remints_identity_and_preserves_notes_and_position() {
    let project = project_tracks(&[TrackKind::Midi, TrackKind::Midi]);
    let source_track = project.tracks[0].id;
    let destination_track = project.tracks[1].id;
    let source = note_clip(7.5);
    let source_id = source.id;
    let mut source_editor = TimelineEditorState::default();
    Arc::make_mut(&mut source_editor.timeline)
        .ensure(source_track)
        .note_clips
        .push(source);
    source_editor
        .selected_clips
        .insert(ArrangementSelection::NoteClip {
            track_id: source_track,
            clip_id: source_id,
        });
    let mut clipboard = ClipClipboard::default();

    copy(
        &mut source_editor,
        &project,
        &mut clipboard,
        ArrangementCtx::default(),
    );
    let mut destination_editor = TimelineEditorState {
        selected_track: Some(destination_track),
        ..TimelineEditorState::default()
    };
    let (action, _) = paste(
        &mut destination_editor,
        &project,
        &mut clipboard,
        ArrangementCtx {
            samples_per_beat: 100.0,
            ..ArrangementCtx::default()
        },
    );

    assert!(action.mark_dirty);
    let pasted = &destination_editor
        .timeline
        .get(destination_track)
        .unwrap()
        .note_clips[0];
    assert_ne!(pasted.id, source_id);
    assert_eq!(pasted.position_beats, 7.5);
    assert_eq!(pasted.notes[0].start_beat, 0.25);
    assert!(pasted.selected_notes.is_empty());
}

#[test]
fn multi_track_paste_maps_relative_tracks_atomically_and_preserves_clip_timing() {
    let project = project_tracks(&[
        TrackKind::Audio,
        TrackKind::Midi,
        TrackKind::Audio,
        TrackKind::Midi,
    ]);
    let source_audio_track = project.tracks[0].id;
    let source_midi_track = project.tracks[1].id;
    let destination_audio_track = project.tracks[2].id;
    let destination_midi_track = project.tracks[3].id;
    let audio = audio_clip(200);
    let second_audio = audio_clip(500);
    let midi = note_clip(3.0);
    let mut source_editor = TimelineEditorState::default();
    let source_content = Arc::make_mut(&mut source_editor.timeline);
    source_content
        .ensure(source_audio_track)
        .clips
        .extend([audio.clone(), second_audio.clone()]);
    source_content
        .ensure(source_midi_track)
        .note_clips
        .push(midi.clone());
    for clip in [&audio, &second_audio] {
        source_editor
            .selected_clips
            .insert(ArrangementSelection::AudioClip {
                track_id: source_audio_track,
                clip_id: clip.id,
            });
    }
    source_editor
        .selected_clips
        .insert(ArrangementSelection::NoteClip {
            track_id: source_midi_track,
            clip_id: midi.id,
        });
    let mut clipboard = ClipClipboard::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..ArrangementCtx::default()
    };
    copy(&mut source_editor, &project, &mut clipboard, ctx);

    let mut destination_editor = TimelineEditorState {
        selected_track: Some(destination_audio_track),
        ..TimelineEditorState::default()
    };
    let (action, _) = paste(&mut destination_editor, &project, &mut clipboard, ctx);

    assert!(action.mark_dirty);
    let mut audio_positions: Vec<_> = destination_editor
        .timeline
        .get(destination_audio_track)
        .unwrap()
        .clips
        .iter()
        .map(|clip| clip.position)
        .collect();
    audio_positions.sort_unstable();
    assert_eq!(audio_positions, vec![200, 500]);
    assert_eq!(
        destination_editor
            .timeline
            .get(destination_midi_track)
            .unwrap()
            .note_clips[0]
            .position_beats,
        3.0
    );
}

#[test]
fn incompatible_or_truncated_mapping_fails_without_changing_timeline_or_clipboard() {
    let project = project_tracks(&[TrackKind::Audio, TrackKind::Midi, TrackKind::Audio]);
    let source_audio_track = project.tracks[0].id;
    let source_midi_track = project.tracks[1].id;
    let audio = audio_clip(200);
    let midi = note_clip(3.0);
    let mut source_editor = TimelineEditorState::default();
    Arc::make_mut(&mut source_editor.timeline)
        .ensure(source_audio_track)
        .clips
        .push(audio.clone());
    Arc::make_mut(&mut source_editor.timeline)
        .ensure(source_midi_track)
        .note_clips
        .push(midi.clone());
    source_editor.selected_clips.extend([
        ArrangementSelection::AudioClip {
            track_id: source_audio_track,
            clip_id: audio.id,
        },
        ArrangementSelection::NoteClip {
            track_id: source_midi_track,
            clip_id: midi.id,
        },
    ]);
    let mut clipboard = ClipClipboard::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..ArrangementCtx::default()
    };
    copy(&mut source_editor, &project, &mut clipboard, ctx);
    let clipboard_ids: Vec<_> = clipboard
        .clips
        .iter()
        .map(|entry| entry.source_track())
        .collect();

    let mut destination_editor = TimelineEditorState {
        selected_track: Some(project.tracks[1].id),
        ..TimelineEditorState::default()
    };
    let (action, engine) = paste(&mut destination_editor, &project, &mut clipboard, ctx);

    assert!(!action.mark_dirty);
    assert!(action.status.unwrap().contains("wrong Clip type"));
    assert!(destination_editor.timeline.by_track.is_empty());
    assert!(engine.0.is_empty());
    assert_eq!(
        clipboard
            .clips
            .iter()
            .map(|entry| entry.source_track())
            .collect::<Vec<_>>(),
        clipboard_ids
    );

    destination_editor.selected_track = Some(project.tracks[2].id);
    let (action, engine) = paste(&mut destination_editor, &project, &mut clipboard, ctx);
    assert!(!action.mark_dirty);
    assert!(action.status.unwrap().contains("runs past"));
    assert!(destination_editor.timeline.by_track.is_empty());
    assert!(engine.0.is_empty());
}

#[test]
fn cross_section_paste_uses_selected_section_and_keeps_out_of_bounds_material() {
    let project = project_tracks(&[TrackKind::Audio, TrackKind::Audio]);
    let source_track = project.tracks[0].id;
    let destination_track = project.tracks[1].id;
    let source_clip = audio_clip(600);
    let source_id = source_clip.id;
    let mut source_section = Section::new(0);
    Arc::make_mut(&mut source_section.timeline)
        .ensure(source_track)
        .clips
        .push(source_clip);
    let mut destination_section = Section::new(1);
    destination_section.length_beats = 4.0;
    let mut clipboard = ClipClipboard::default();
    let ctx = ArrangementCtx {
        samples_per_beat: 100.0,
        ..ArrangementCtx::default()
    };

    let mut source_editor = SectionTimelineEditor::default();
    source_editor.load(Arc::clone(&source_section.timeline), Some(source_track));
    source_editor
        .editor_mut()
        .selected_clips
        .insert(ArrangementSelection::AudioClip {
            track_id: source_track,
            clip_id: source_id,
        });
    copy(source_editor.editor_mut(), &project, &mut clipboard, ctx);

    let mut destination_editor = SectionTimelineEditor::default();
    destination_editor.load(
        Arc::clone(&destination_section.timeline),
        Some(destination_track),
    );
    let (action, _) = paste(
        destination_editor.editor_mut(),
        &project,
        &mut clipboard,
        ctx,
    );
    destination_section.timeline = Arc::clone(&destination_editor.editor().timeline);

    assert!(action.mark_dirty);
    let pasted = &destination_section
        .timeline
        .get(destination_track)
        .unwrap()
        .clips[0];
    assert_eq!(pasted.position, 600);
    assert_ne!(pasted.id, source_id);
    assert!(!destination_section.contains_playable_beat(6.0));
    assert_eq!(
        source_section.timeline.get(source_track).unwrap().clips[0].id,
        source_id
    );
}
