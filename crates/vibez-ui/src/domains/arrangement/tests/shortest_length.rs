//! An edge drag shrinks a Clip as far as the shortest length and no further.

use super::*;

const CTX: ArrangementCtx = ArrangementCtx {
    samples_per_beat: 800.0,
    playhead_samples: 0,
    playhead_beats: 0.0,
};

fn resize(a: &mut ArrangementFixture, anchor: ArrangementSelection, beats: f64) {
    a.update(
        ArrangementMsg::ResizeSelectedClips {
            anchor,
            new_duration_beats: beats,
        },
        &mut RecordingEngine::default(),
        CTX,
    );
}

#[test]
fn a_clip_shrinks_to_a_thirty_second_on_the_finest_grid() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, clip_id) = add_audio_clip(&mut a, 0, 0, 1_600);

    resize(
        &mut a,
        ArrangementSelection::AudioClip { track_id, clip_id },
        0.125,
    );

    assert_eq!(a.tracks[0].clips[0].duration, 100);
}

#[test]
fn every_selected_clip_stops_at_the_shortest_length() {
    let mut a = arrangement_with_tracks(1);
    let (track_id, long) = add_audio_clip(&mut a, 0, 0, 3_200);
    let (_, short) = add_audio_clip(&mut a, 0, 4_000, 1_600);
    let anchor = ArrangementSelection::AudioClip {
        track_id,
        clip_id: long,
    };
    a.selected_clips.insert(anchor);
    a.selected_clips.insert(ArrangementSelection::AudioClip {
        track_id,
        clip_id: short,
    });

    resize(&mut a, anchor, 0.5);

    let duration = |id: ClipId| {
        a.tracks[0]
            .clips
            .iter()
            .find(|clip| clip.id == id)
            .unwrap()
            .duration
    };
    assert_eq!(duration(long), 400);
    assert_eq!(duration(short), 100);
}
