//! Random sequences of Audio Clip edits survive save and reopen unchanged.

use std::collections::HashSet;
use std::sync::Arc;

use proptest::prelude::*;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, TrackId};
use vibez_project::TimelineLocation;

use super::test_support::{app, run_until_idle};
use super::{load_project_async, save_project_async, App};
use crate::domains::arrangement::ArrangementMsg;
use crate::domains::project::ProjectMsg;
use crate::message::Message;
use crate::state::{
    ArrangementSelection, AudioClipInspectorField, AudioClipRotaryField, ProjectTrack, SnapGrid,
    UiClip,
};

/// Producer edits on Audio Clips. Clip indexes wrap around the Clips on the
/// lane, sorted by position, so every generated edit targets a real Clip.
#[derive(Debug, Clone)]
enum Edit {
    Split { clip: usize, at: f64 },
    Join { clip: usize },
    Duplicate { clip: usize },
    Move { clip: usize, beats: f64 },
    Quantize { clip: usize },
    Warp { clip: usize, source_bpm: f64 },
    Transpose { clip: usize, semitones: i8 },
    Undo,
    Redo,
}

fn edit() -> impl Strategy<Value = Edit> {
    let clip = 0usize..8;
    prop_oneof![
        (clip.clone(), 0.1f64..0.9).prop_map(|(clip, at)| Edit::Split { clip, at }),
        clip.clone().prop_map(|clip| Edit::Join { clip }),
        clip.clone().prop_map(|clip| Edit::Duplicate { clip }),
        (clip.clone(), -8i32..=8).prop_map(|(clip, halves)| Edit::Move {
            clip,
            beats: f64::from(halves) * 0.5,
        }),
        clip.clone().prop_map(|clip| Edit::Quantize { clip }),
        (
            clip.clone(),
            prop::sample::select(vec![100.0, 140.0, 170.0])
        )
            .prop_map(|(clip, source_bpm)| Edit::Warp { clip, source_bpm }),
        (clip, -12i8..=12).prop_map(|(clip, semitones)| Edit::Transpose { clip, semitones }),
        Just(Edit::Undo),
        Just(Edit::Redo),
    ]
}

/// Half a second of decaying noise bursts, so Quantize finds transients.
fn break_audio(sample_rate: u32) -> Arc<DecodedAudio> {
    let frames = sample_rate as usize / 2;
    let mut samples = vec![0.0f32; frames];
    for hit in (0..4).map(|index| 500 + index * frames / 4) {
        for (offset, sample) in samples[hit..(hit + 2_000).min(frames)]
            .iter_mut()
            .enumerate()
        {
            let noise = if (offset * 7_919) % 13 < 6 { 1.0 } else { -1.0 };
            *sample = noise * 0.5 * (-(offset as f32) / 400.0).exp();
        }
    }
    Arc::new(DecodedAudio {
        channels: vec![samples],
        sample_rate,
    })
}

/// An App with one Audio track holding two Clips of a staged break.
fn project_with_breaks(directory: &std::path::Path) -> (App, TrackId) {
    let mut app = app();
    app.state.transport.bpm = 120.0;
    let source_path = directory.join("break.wav");
    vibez_audio_io::file_io::write_wav_file(
        &source_path,
        &break_audio(app.state.transport.sample_rate),
    )
    .unwrap();
    // Imports play what was decoded from the file, so reopen decodes the same samples.
    let audio = Arc::new(vibez_audio_io::file_io::decode_audio_file(&source_path).unwrap());
    let source = vibez_project::project_format_v1::stage_local_file(&source_path).unwrap();
    let track = ProjectTrack::new(TrackId::new(), "Breaks".into(), 0);
    let track_id = track.id;
    Arc::make_mut(&mut app.state.project_tracks)
        .tracks
        .push(track);
    let frames = audio.num_frames() as u64;
    for position in [0, frames] {
        let clip = UiClip {
            name: "break.wav".into(),
            audio: Arc::clone(&audio),
            source: Some(source.clone()),
            ..UiClip::test_clip(position, frames)
        };
        app.state.arrange_content_mut(track_id).clips.push(clip);
    }
    (app, track_id)
}

fn sorted_clips(app: &App, track_id: TrackId) -> Vec<UiClip> {
    let mut clips = app
        .state
        .arrange_content(track_id)
        .map(|content| content.clips.clone())
        .unwrap_or_default();
    clips.sort_by_key(|clip| (clip.position, clip.id.raw()));
    clips
}

fn select(app: &mut App, track_id: TrackId, clips: &[&UiClip]) {
    app.state.arrangement.selected_clips = clips
        .iter()
        .map(|clip| ArrangementSelection::AudioClip {
            track_id,
            clip_id: clip.id,
        })
        .collect::<HashSet<_>>();
}

async fn apply(app: &mut App, track_id: TrackId, edit: &Edit) {
    let clips = sorted_clips(app, track_id);
    let pick = |index: usize| clips.get(index % clips.len().max(1));
    let samples_per_beat = 60.0 * f64::from(app.state.transport.sample_rate) / 120.0;
    let messages = match *edit {
        Edit::Undo => vec![Message::Project(ProjectMsg::Undo)],
        Edit::Redo => vec![Message::Project(ProjectMsg::Redo)],
        Edit::Split { clip, at } => pick(clip)
            .map(|clip| {
                let offset = (clip.duration as f64 * at) as u64;
                vec![Message::Arrangement(ArrangementMsg::SplitAudioClip {
                    track_id,
                    clip_id: clip.id,
                    split_position: clip.position + offset.max(1),
                })]
            })
            .unwrap_or_default(),
        Edit::Join { clip } => {
            if clips.len() < 2 {
                return;
            }
            let first = clip % (clips.len() - 1);
            select(app, track_id, &[&clips[first], &clips[first + 1]]);
            vec![Message::Arrangement(ArrangementMsg::JoinSelectedClips)]
        }
        Edit::Duplicate { clip } => {
            let Some(clip) = pick(clip) else { return };
            select(app, track_id, &[clip]);
            vec![Message::Arrangement(ArrangementMsg::DuplicateSelectedClip)]
        }
        Edit::Move { clip, beats } => {
            let Some(clip) = pick(clip) else { return };
            select(app, track_id, &[clip]);
            let position_beats = clip.position as f64 / samples_per_beat;
            vec![Message::Arrangement(ArrangementMsg::MoveSelectedClips {
                anchor: clip.id,
                new_position_beats: (position_beats + beats).max(0.0),
                target_track: track_id,
            })]
        }
        Edit::Quantize { clip } => pick(clip)
            .map(|clip| {
                vec![Message::QuantizeAudioClipAt {
                    track_id,
                    clip_id: clip.id,
                    grid: SnapGrid::SIXTEENTH,
                }]
            })
            .unwrap_or_default(),
        Edit::Warp { clip, source_bpm } => pick(clip)
            .map(|clip| {
                vec![
                    Message::Arrangement(ArrangementMsg::AudioClipInspectorInputChanged {
                        clip_id: clip.id,
                        field: AudioClipInspectorField::SourceBpm,
                        text: source_bpm.to_string(),
                    }),
                    Message::Arrangement(ArrangementMsg::SubmitAudioClipInspectorField {
                        track_id,
                        clip_id: clip.id,
                        field: AudioClipInspectorField::SourceBpm,
                    }),
                    Message::WarpClipToProject {
                        location: TimelineLocation::Arrange,
                        track_id,
                        clip_id: clip.id,
                    },
                ]
            })
            .unwrap_or_default(),
        Edit::Transpose { clip, semitones } => pick(clip)
            .map(|clip| {
                vec![Message::Arrangement(
                    ArrangementMsg::SetAudioClipRotaryValue {
                        track_id,
                        clip_id: clip.id,
                        field: AudioClipRotaryField::Transpose,
                        value: f32::from(semitones),
                    },
                )]
            })
            .unwrap_or_default(),
    };
    for message in messages {
        run_until_idle(app, message).await;
    }
}

/// What a producer would notice about a lane: where each Clip sits, how long
/// it is, and that its audio is backed by saved media.
fn lane_summary(app: &App, track_id: TrackId) -> Vec<(ClipId, u64, u64, bool)> {
    sorted_clips(app, track_id)
        .iter()
        .map(|clip| (clip.id, clip.position, clip.duration, clip.source.is_some()))
        .collect()
}

fn render_master(app: &App) -> Vec<f32> {
    let project = app.project_for_offline_render();
    let end = project
        .arrange
        .clips
        .iter()
        .map(|clip| clip.position + clip.duration)
        .max()
        .unwrap_or(0);
    if end == 0 {
        return Vec::new();
    }
    let assets = app.collect_bounce_assets();
    let request = vibez_engine::render::BounceRequest {
        tracks: project.tracks,
        master: project.master,
        buses: project.buses,
        audio_clips: project.arrange.clips,
        note_clips: project.arrange.note_clips,
        clip_audio: assets.clips,
        sampler_audio: assets.samplers,
        drum_pad_audio: assets.pads,
        mode: vibez_engine::render::BounceMode::Master,
        range_samples: (0, end),
        bpm: app.state.transport.bpm,
        sample_rate: app.state.transport.sample_rate,
        swing: project.swing,
    };
    vibez_engine::render::render_offline(&request)
        .audio
        .channels
        .concat()
}

async fn assert_survives_save_and_reopen(edits: &[Edit]) -> Result<(), TestCaseError> {
    let directory = tempfile::tempdir().unwrap();
    let project_path = directory.path().join("round-trip.vzp");
    let (mut app, track_id) = project_with_breaks(directory.path());
    for edit in edits {
        apply(&mut app, track_id, edit).await;
    }
    let before = lane_summary(&app, track_id);
    let heard_before = render_master(&app);
    prop_assert!(
        before.iter().all(|(.., has_media)| *has_media),
        "a Clip has no saved media: {before:?}"
    );

    save_project_async(project_path.clone(), None, app.project_for_save())
        .await
        .unwrap();
    let loaded = load_project_async(project_path, None, app.state.transport.sample_rate)
        .await
        .unwrap();
    prop_assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let mut reopened = super::test_support::app();
    reopened.state.transport.bpm = app.state.transport.bpm;
    reopened.rebuild_from_loaded_project(loaded);

    prop_assert_eq!(lane_summary(&reopened, track_id), before);
    let heard_after = render_master(&reopened);
    prop_assert_eq!(heard_after.len(), heard_before.len());
    let worst = heard_before
        .iter()
        .zip(&heard_after)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    // Generated audio is saved as 16-bit WAV, so allow its rounding error.
    prop_assert!(worst < 1e-3, "reopened render differs by {worst}");
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        ..ProptestConfig::default()
    })]

    #[test]
    fn clip_edits_survive_save_and_reopen(edits in prop::collection::vec(edit(), 1..8)) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(assert_survives_save_and_reopen(&edits))?;
    }
}
