//! Clip destinations share the loop note recorder and the Project Track input path.
use super::loop_record::{
    CompletedLoopRecording, LoopRecordCountIn, LoopRecordMode, LoopRecordQuantization,
    LoopRecordState,
};
use super::LauncherClip;
use crate::state::{ArrangementTimeline, UiNoteClip};
use std::sync::Arc;
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RecordLength {
    #[default]
    Free,
    Bars(u8),
}
impl RecordLength {
    pub const ALL: [Self; 6] = [
        Self::Free,
        Self::Bars(1),
        Self::Bars(2),
        Self::Bars(4),
        Self::Bars(8),
        Self::Bars(16),
    ];
    pub fn beats(self) -> Option<f64> {
        match self {
            Self::Free => None,
            Self::Bars(bars) => Some(f64::from(bars) * 4.0),
        }
    }
}
impl std::fmt::Display for RecordLength {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Free => write!(f, "Free length"),
            Self::Bars(bars) => write!(f, "{bars} bar loop"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipRecordMsg {
    Slot(TrackId, u32),
    SetLength(RecordLength),
    SetMode(LoopRecordMode),
    SetCountIn(LoopRecordCountIn),
    SetQuantization(LoopRecordQuantization),
}
#[derive(Debug, Default)]
pub struct ClipRecordState {
    pub length: RecordLength,
    pub notes: LoopRecordState<ClipId>,
    pub session: Option<ClipRecordSession>,
}
#[derive(Debug)]
pub struct ClipRecordSession {
    pub original: Option<LauncherClip>,
    pub working: LauncherClip,
    pub audio: bool,
    pub length_samples: Option<u64>,
    pub start: Option<u64>,
    pub output_start: Option<u64>,
    pub pass: u64,
    pub stop: Option<u64>,
    pub sample_rate: u32,
    pub bpm: f64,
}
impl ClipRecordState {
    pub fn is_active(&self) -> bool {
        self.session.is_some()
    }
}

pub fn empty_midi_clip(
    id: ClipId,
    track_id: TrackId,
    row: u32,
    name: String,
    beats: f64,
) -> LauncherClip {
    let mut timeline = ArrangementTimeline::default();
    timeline.ensure(track_id).note_clips.push(UiNoteClip {
        id,
        name,
        position_beats: 0.0,
        duration_beats: beats,
        notes: vec![],
        selected_notes: Default::default(),
        start_marker_beats: 0.0,
        loop_enabled: true,
        loop_start_beats: 0.0,
        loop_end_beats: beats,
        groove_grid: Default::default(),
    });
    LauncherClip {
        id,
        track_id,
        row,
        timeline: Arc::new(timeline),
    }
}

pub fn normalize_midi_clip(clip: &LauncherClip, beats: f64) -> LauncherClip {
    let mut normalized = clip.clone();
    let content = Arc::make_mut(&mut normalized.timeline).ensure(clip.track_id);
    for source in &mut content.note_clips {
        source.notes = super::capture::captured_visible_notes(source, 0.0, beats);
        source.position_beats = 0.0;
        source.duration_beats = beats;
        source.start_marker_beats = 0.0;
        source.loop_start_beats = 0.0;
        source.loop_end_beats = beats;
    }
    normalized
}

pub fn apply_midi_take(
    base: &LauncherClip,
    take: &CompletedLoopRecording<ClipId>,
    beats: f64,
) -> LauncherClip {
    let mut clip = base.clone();
    let content = Arc::make_mut(&mut clip.timeline).ensure(clip.track_id);
    let target = &mut content.note_clips[0];
    let was_empty = target.notes.is_empty();
    target.duration_beats = beats;
    target.loop_end_beats = beats;
    target.notes.retain(|note| {
        !take
            .replace_ranges
            .iter()
            .any(|(from, to)| note.start_beat >= *from && note.start_beat < *to)
    });
    // Pocket is a property of the single editable MIDI part, including overdubbed notes.
    if was_empty {
        if let Some(note) = take.notes.first() {
            target.groove_grid = note.groove_grid;
        }
    }
    target.notes.extend(take.notes.iter().map(|note| MidiNote {
        pitch: note.pitch,
        velocity: note.velocity,
        start_beat: note.start_beat,
        duration_beats: note.duration_beats,
    }));
    target.notes.sort_by(|a, b| {
        a.start_beat
            .total_cmp(&b.start_beat)
            .then(a.pitch.cmp(&b.pitch))
    });

    clip
}

pub fn latest_audio_pass(frames: &[[f32; 2]], length: usize) -> Vec<[f32; 2]> {
    let mut result = vec![[0.0; 2]; length.max(1)];
    for (index, frame) in frames.iter().enumerate() {
        let slot = index % result.len();
        result[slot] = *frame;
    }
    result
}

pub fn recorded_audio_window(
    frames: &[[f32; 2]],
    bridge_start: u64,
    output_start: u64,
    duration: u64,
    loop_length: Option<u64>,
) -> Option<Vec<[f32; 2]>> {
    let start = output_start.saturating_sub(bridge_start) as usize;
    if start >= frames.len() || duration == 0 {
        return None;
    }
    let end = start.saturating_add(duration as usize).min(frames.len());
    let window = &frames[start..end];
    Some(loop_length.map_or_else(
        || window.to_vec(),
        |length| latest_audio_pass(window, length as usize),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::loop_record::{LoopRecordInput, RecordedLoopNote};
    use super::*;
    use vibez_core::perform::GrooveGrid;

    #[test]
    fn audio_window_excludes_count_in_and_tail_and_replaces_each_loop_pass() {
        let frames: Vec<_> = (0..14).map(|n| [n as f32; 2]).collect();
        let take = recorded_audio_window(&frames, 100, 103, 6, Some(4)).unwrap();
        assert_eq!(take, vec![[7.0; 2], [8.0; 2], [5.0; 2], [6.0; 2]]);
        assert_eq!(
            recorded_audio_window(&frames, 100, 103, 6, None).unwrap(),
            frames[3..9]
        );
        assert!(recorded_audio_window(&frames, 100, 115, 4, None).is_none());
    }

    #[test]
    fn midi_replace_clears_only_crossed_starts_and_preserves_groove_and_one_shot() {
        let id = ClipId::new();
        let track = TrackId::new();
        let mut base = empty_midi_clip(id, track, 0, "Hat".into(), 4.0);
        let target = &mut Arc::make_mut(&mut base.timeline).ensure(track).note_clips[0];
        target.loop_enabled = false;
        target.notes = vec![
            MidiNote {
                pitch: 40,
                velocity: 100,
                start_beat: 0.5,
                duration_beats: 0.1,
            },
            MidiNote {
                pitch: 41,
                velocity: 100,
                start_beat: 3.0,
                duration_beats: 0.1,
            },
        ];
        let take = CompletedLoopRecording {
            target_id: id,
            track_id: track,
            replace_ranges: vec![(0.0, 2.0)],
            notes: vec![RecordedLoopNote {
                pitch: 42,
                velocity: 90,
                start_beat: 1.0,
                duration_beats: 0.25,
                groove_grid: GrooveGrid::Sixteenth,
            }],
        };
        let recorded = apply_midi_take(&normalize_midi_clip(&base, 4.0), &take, 4.0);
        let clips = &recorded.timeline.get(track).unwrap().note_clips;
        assert_eq!(clips.len(), 1);
        assert_eq!(
            clips[0]
                .notes
                .iter()
                .map(|note| note.pitch)
                .collect::<Vec<_>>(),
            vec![42, 41]
        );
        assert_eq!(clips[0].groove_grid, GrooveGrid::Off);
        assert!(clips.iter().all(|clip| !clip.loop_enabled));
    }

    #[test]
    fn fixed_recording_carries_held_notes_and_keeps_replace_or_overdub_across_passes() {
        for mode in [LoopRecordMode::Replace, LoopRecordMode::Overdub] {
            let mut recorder = LoopRecordState::<ClipId>::default();
            recorder.mode = mode;
            let id = ClipId::new();
            let track = TrackId::new();
            recorder.sync_clock(false, 60.0, 100);
            recorder.request_start(id, track, false, 4.0).unwrap();
            recorder.start(id, track, 0, 0);
            recorder.input_note(LoopRecordInput {
                target_id: Some(id),
                track_id: track,
                pitch: 60,
                velocity: 90,
                on: true,
                effective_at_samples: 350,
                local_position_samples: Some(350),
            });
            recorder.request_stop();
            let first = recorder.next_pass(400).unwrap();
            assert_eq!(first.notes[0].duration_beats, 0.5);
            assert_eq!(
                first.replace_ranges.is_empty(),
                mode == LoopRecordMode::Overdub
            );
            assert_eq!(
                recorder.phase,
                super::super::loop_record::LoopRecordPhase::Stopping
            );
            assert!(recorder.snapshot(400, 0).unwrap().notes.is_empty());
            recorder.input_note(LoopRecordInput {
                target_id: Some(id),
                track_id: track,
                pitch: 60,
                velocity: 0,
                on: false,
                effective_at_samples: 450,
                local_position_samples: Some(50),
            });
            let second = recorder.snapshot(500, 100).unwrap();
            assert_eq!(second.notes[0].start_beat, 0.0);
            assert_eq!(second.notes[0].duration_beats, 0.5);
            assert_eq!(
                second.replace_ranges.is_empty(),
                mode == LoopRecordMode::Overdub
            );
        }
    }
}
