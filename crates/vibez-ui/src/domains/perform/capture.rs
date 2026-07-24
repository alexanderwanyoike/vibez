//! Runtime Capture log and pure Section-to-Arrange materialization.
//!
//! The audio engine owns effective timestamps. This module snapshots the
//! canonical Section source at those boundaries, then creates independent
//! linear Arrange clips only after the engine confirms Capture stop.

use std::collections::HashMap;
use std::sync::Arc;

use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

use crate::state::{ArrangementTimeline, TrackTimelineContent, UiClip, UiNoteClip};

use super::{PerformAction, Section};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapturePhase {
    #[default]
    Idle,
    Starting,
    Recording,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureMsg {
    Toggle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureAction {
    Start,
    Stop,
}

#[derive(Debug, Clone)]
pub struct CapturedSectionSource {
    pub name: String,
    pub length_beats: f64,
    pub looping: bool,
    pub timeline: Arc<ArrangementTimeline>,
}

impl CapturedSectionSource {
    pub fn from_section(section: &Section) -> Self {
        Self {
            name: section.name.clone(),
            length_beats: section.length_beats,
            looping: section.looping,
            timeline: Arc::clone(&section.timeline),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct CaptureClock {
    arrange_start_samples: u64,
    sample_rate: u32,
    bpm: f64,
}

#[derive(Debug, Clone)]
struct ActiveSpan {
    source: CapturedSectionSource,
    effective_start_samples: u64,
    source_start_samples: u64,
}

#[derive(Debug, Clone)]
struct CapturedSectionSpan {
    source: CapturedSectionSource,
    effective_start_samples: u64,
    effective_end_samples: u64,
    source_start_samples: u64,
}

#[derive(Debug)]
struct CaptureSession {
    clock: CaptureClock,
    engine_start_samples: u64,
    active: Option<ActiveSpan>,
    spans: Vec<CapturedSectionSpan>,
}

#[derive(Debug)]
pub struct CompletedCapture {
    clock: CaptureClock,
    engine_start_samples: u64,
    engine_end_samples: u64,
    spans: Vec<CapturedSectionSpan>,
}

#[derive(Debug, Default)]
pub struct MaterializedCapture {
    pub arrange_start_samples: u64,
    pub arrange_end_samples: u64,
    pub by_track: HashMap<TrackId, TrackTimelineContent>,
    pub(crate) samples_per_beat: f64,
}

impl MaterializedCapture {
    pub fn is_empty(&self) -> bool {
        self.by_track.values().all(|content| {
            content.clips.is_empty()
                && content.note_clips.is_empty()
                && content.automation.is_empty()
        })
    }

    /// Card 17 is deliberately safe only for an empty destination interval.
    /// Replacement and lane surgery arrive in Card 18.
    pub fn target_interval_is_empty(
        &self,
        arrange: &ArrangementTimeline,
        perform_track_ids: &[TrackId],
    ) -> bool {
        if self.arrange_end_samples <= self.arrange_start_samples {
            return true;
        }
        perform_track_ids.iter().all(|track_id| {
            arrange.get(*track_id).is_none_or(|existing| {
                let audio_clear = existing.clips.iter().all(|clip| {
                    clip.position.saturating_add(clip.duration) <= self.arrange_start_samples
                        || clip.position >= self.arrange_end_samples
                });
                let note_clear = existing.note_clips.iter().all(|clip| {
                    let start = (clip.position_beats * self.samples_per_beat)
                        .round()
                        .max(0.0) as u64;
                    let end = ((clip.position_beats + clip.duration_beats) * self.samples_per_beat)
                        .round()
                        .max(0.0) as u64;
                    end <= self.arrange_start_samples || start >= self.arrange_end_samples
                });
                audio_clear && note_clear && existing.automation.is_empty()
            })
        })
    }
}

impl CompletedCapture {
    pub fn materialize(&self) -> MaterializedCapture {
        let mut result = MaterializedCapture {
            arrange_start_samples: self.clock.arrange_start_samples,
            arrange_end_samples: self.clock.arrange_start_samples.saturating_add(
                self.engine_end_samples
                    .saturating_sub(self.engine_start_samples),
            ),
            by_track: HashMap::new(),
            samples_per_beat: samples_per_beat(self.clock.sample_rate, self.clock.bpm),
        };
        let samples_per_beat = result.samples_per_beat;
        if samples_per_beat <= 0.0 {
            return result;
        }

        for span in &self.spans {
            let span_length = span
                .effective_end_samples
                .saturating_sub(span.effective_start_samples);
            let section_length = (span.source.length_beats * samples_per_beat)
                .round()
                .max(1.0) as u64;
            let mut remaining = span_length;
            let mut source_cursor = span.source_start_samples.min(section_length);
            let mut destination_cursor = self.clock.arrange_start_samples.saturating_add(
                span.effective_start_samples
                    .saturating_sub(self.engine_start_samples),
            );

            while remaining > 0 && source_cursor < section_length {
                let segment_length = remaining.min(section_length - source_cursor);
                append_timeline_window(
                    &mut result.by_track,
                    &span.source,
                    source_cursor,
                    source_cursor + segment_length,
                    destination_cursor,
                    samples_per_beat,
                );
                remaining -= segment_length;
                destination_cursor = destination_cursor.saturating_add(segment_length);
                if remaining == 0 || !span.source.looping {
                    break;
                }
                source_cursor = 0;
            }
        }

        for content in result.by_track.values_mut() {
            content.clips.sort_by_key(|clip| clip.position);
            content
                .note_clips
                .sort_by(|left, right| left.position_beats.total_cmp(&right.position_beats));
        }
        result
    }
}

#[derive(Debug, Default)]
pub struct CaptureState {
    pub phase: CapturePhase,
    prepared_clock: Option<CaptureClock>,
    session: Option<CaptureSession>,
}

impl CaptureState {
    pub fn is_active(&self) -> bool {
        self.phase != CapturePhase::Idle
    }

    pub fn arrange_start_samples(&self) -> Option<u64> {
        self.session
            .as_ref()
            .map(|session| session.clock.arrange_start_samples)
            .or_else(|| self.prepared_clock.map(|clock| clock.arrange_start_samples))
    }

    pub fn update(&mut self, msg: CaptureMsg) -> PerformAction {
        match (msg, self.phase) {
            (CaptureMsg::Toggle, CapturePhase::Idle) => {
                self.phase = CapturePhase::Starting;
                PerformAction {
                    capture: Some(CaptureAction::Start),
                    ..PerformAction::default()
                }
            }
            (CaptureMsg::Toggle, CapturePhase::Recording) => {
                self.phase = CapturePhase::Stopping;
                PerformAction {
                    capture: Some(CaptureAction::Stop),
                    ..PerformAction::default()
                }
            }
            (CaptureMsg::Toggle, CapturePhase::Starting | CapturePhase::Stopping) => {
                PerformAction::default()
            }
        }
    }

    pub fn prepare(&mut self, arrange_start_samples: u64, sample_rate: u32, bpm: f64) {
        self.prepared_clock = Some(CaptureClock {
            arrange_start_samples,
            sample_rate,
            bpm,
        });
    }

    pub fn start(
        &mut self,
        effective_at_samples: u64,
        active: Option<(CapturedSectionSource, u64)>,
    ) {
        if self.phase != CapturePhase::Starting {
            return;
        }
        let Some(clock) = self.prepared_clock.take() else {
            self.cancel();
            return;
        };
        self.session = Some(CaptureSession {
            clock,
            engine_start_samples: effective_at_samples,
            active: active.map(|(source, source_start_samples)| ActiveSpan {
                source,
                effective_start_samples: effective_at_samples,
                source_start_samples,
            }),
            spans: Vec::new(),
        });
        self.phase = CapturePhase::Recording;
    }

    pub fn transition(&mut self, source: CapturedSectionSource, effective_at_samples: u64) {
        let Some(session) = &mut self.session else {
            return;
        };
        close_active_span(session, effective_at_samples);
        session.active = Some(ActiveSpan {
            source,
            effective_start_samples: effective_at_samples,
            source_start_samples: 0,
        });
    }

    pub fn finish(&mut self, effective_at_samples: u64) -> Option<CompletedCapture> {
        let Some(mut session) = self.session.take() else {
            self.cancel();
            return None;
        };
        close_active_span(&mut session, effective_at_samples);
        self.phase = CapturePhase::Idle;
        self.prepared_clock = None;
        Some(CompletedCapture {
            clock: session.clock,
            engine_start_samples: session.engine_start_samples,
            engine_end_samples: effective_at_samples,
            spans: session.spans,
        })
    }

    pub fn cancel(&mut self) {
        self.phase = CapturePhase::Idle;
        self.prepared_clock = None;
        self.session = None;
    }
}

fn close_active_span(session: &mut CaptureSession, effective_end_samples: u64) {
    let Some(active) = session.active.take() else {
        return;
    };
    if effective_end_samples > active.effective_start_samples {
        session.spans.push(CapturedSectionSpan {
            source: active.source,
            effective_start_samples: active.effective_start_samples,
            effective_end_samples,
            source_start_samples: active.source_start_samples,
        });
    }
}

fn samples_per_beat(sample_rate: u32, bpm: f64) -> f64 {
    if bpm > 0.0 {
        60.0 * sample_rate as f64 / bpm
    } else {
        0.0
    }
}

fn append_timeline_window(
    destination: &mut HashMap<TrackId, TrackTimelineContent>,
    source: &CapturedSectionSource,
    window_start_samples: u64,
    window_end_samples: u64,
    destination_start_samples: u64,
    samples_per_beat: f64,
) {
    let window_start_beats = window_start_samples as f64 / samples_per_beat;
    let window_end_beats = window_end_samples as f64 / samples_per_beat;
    let destination_start_beats = destination_start_samples as f64 / samples_per_beat;

    for (track_id, content) in &source.timeline.by_track {
        for clip in &content.clips {
            let overlap_start = clip.position.max(window_start_samples);
            let overlap_end = clip
                .position
                .saturating_add(clip.duration)
                .min(window_end_samples);
            if overlap_end <= overlap_start {
                continue;
            }
            let delta = overlap_start - clip.position;
            let mut fragment = UiClip {
                id: ClipId::new(),
                name: format!("Capture · {} · {}", source.name, clip.name),
                audio: Arc::clone(&clip.audio),
                source: clip.source.clone(),
                position: destination_start_samples + (overlap_start - window_start_samples),
                source_offset: mapped_audio_offset(clip, delta),
                duration: overlap_end - overlap_start,
                loop_enabled: clip.loop_enabled,
                loop_start: clip.loop_start,
                loop_end: clip.loop_end,
                original_bpm: clip.original_bpm,
                warped: clip.warped,
                warped_to_bpm: clip.warped_to_bpm,
                original_audio: clip.original_audio.as_ref().map(Arc::clone),
            };
            if fragment.loop_enabled && fragment.loop_end <= fragment.loop_start {
                fragment.loop_enabled = false;
            }
            destination
                .entry(*track_id)
                .or_default()
                .clips
                .push(fragment);
        }

        for clip in &content.note_clips {
            let clip_end = clip.position_beats + clip.duration_beats;
            let overlap_start = clip.position_beats.max(window_start_beats);
            let overlap_end = clip_end.min(window_end_beats);
            if overlap_end <= overlap_start {
                continue;
            }
            let local_start = overlap_start - clip.position_beats;
            let local_end = overlap_end - clip.position_beats;
            let notes = visible_notes(clip, local_start, local_end);
            let fragment = UiNoteClip {
                id: ClipId::new(),
                name: format!("Capture · {} · {}", source.name, clip.name),
                position_beats: destination_start_beats + (overlap_start - window_start_beats),
                duration_beats: overlap_end - overlap_start,
                notes,
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
                groove_grid: clip.groove_grid,
            };
            destination
                .entry(*track_id)
                .or_default()
                .note_clips
                .push(fragment);
        }
    }
}

fn mapped_audio_offset(clip: &UiClip, timeline_delta: u64) -> u64 {
    let raw = clip.source_offset.saturating_add(timeline_delta);
    if clip.loop_enabled && clip.loop_end > clip.loop_start && raw >= clip.loop_end {
        clip.loop_start + (raw - clip.loop_start) % (clip.loop_end - clip.loop_start)
    } else {
        raw
    }
}

fn visible_notes(clip: &UiNoteClip, local_start: f64, local_end: f64) -> Vec<MidiNote> {
    let looping = clip.loop_enabled && clip.loop_end_beats > clip.loop_start_beats;
    let mut visible = Vec::new();
    for note in &clip.notes {
        let mut occurrence = note.start_beat;
        loop {
            let note_end = occurrence + note.duration_beats;
            let kept_start = occurrence.max(local_start);
            let kept_end = note_end.min(local_end);
            if kept_end > kept_start {
                visible.push(MidiNote {
                    start_beat: kept_start - local_start,
                    duration_beats: kept_end - kept_start,
                    ..*note
                });
            }
            if !looping
                || note.start_beat < clip.loop_start_beats
                || note.start_beat >= clip.loop_end_beats
            {
                break;
            }
            occurrence += clip.loop_end_beats - clip.loop_start_beats;
            if occurrence >= local_end {
                break;
            }
        }
    }
    visible.sort_by(|left, right| {
        left.start_beat
            .total_cmp(&right.start_beat)
            .then(left.pitch.cmp(&right.pitch))
    });
    visible
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::audio_buffer::DecodedAudio;
    use vibez_core::perform::GrooveGrid;

    fn audio_section(name: &str, length_beats: f64, frames: u64) -> Section {
        let mut section = Section::new(0);
        section.name = name.into();
        section.length_beats = length_beats;
        let track_id = TrackId::new();
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .clips
            .push(UiClip {
                id: ClipId::new(),
                name: format!("{name} Audio"),
                audio: Arc::new(DecodedAudio {
                    channels: vec![vec![0.5; frames as usize]],
                    sample_rate: 8,
                }),
                source: None,
                position: 0,
                source_offset: 0,
                duration: frames,
                loop_enabled: false,
                loop_start: 0,
                loop_end: 0,
                original_bpm: None,
                warped: false,
                warped_to_bpm: None,
                original_audio: None,
            });
        section
    }

    fn midi_section(name: &str, track_id: TrackId, pitch: u8) -> Section {
        let mut section = Section::new(0);
        section.name = name.into();
        section.length_beats = 4.0;
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .note_clips
            .push(UiNoteClip {
                id: ClipId::new(),
                name: format!("{name} Notes"),
                position_beats: 0.0,
                duration_beats: 4.0,
                notes: vec![MidiNote {
                    pitch,
                    velocity: 100,
                    start_beat: 0.0,
                    duration_beats: 0.25,
                }],
                selected_notes: Default::default(),
                loop_enabled: false,
                loop_start_beats: 0.0,
                loop_end_beats: 0.0,
                groove_grid: GrooveGrid::Sixteenth,
            });
        section
    }

    #[test]
    fn effective_mid_buffer_boundaries_become_exact_arrange_positions() {
        let first = audio_section("Groove A", 4.0, 16);
        let second = audio_section("Groove B", 4.0, 16);
        let track_id = *first.timeline.by_track.keys().next().unwrap();
        let mut second = second;
        let second_track_id = *second.timeline.by_track.keys().next().unwrap();
        let second_content = Arc::make_mut(&mut second.timeline)
            .by_track
            .remove(&second_track_id)
            .unwrap();
        Arc::make_mut(&mut second.timeline)
            .by_track
            .insert(track_id, second_content);

        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(40, 8, 120.0);
        capture.start(3, Some((CapturedSectionSource::from_section(&first), 0)));
        assert_eq!(capture.arrange_start_samples(), Some(40));
        capture.transition(CapturedSectionSource::from_section(&second), 10);
        let completed = capture.finish(15).unwrap();
        assert_eq!(capture.arrange_start_samples(), None);
        let materialized = completed.materialize();
        let clips = &materialized.by_track[&track_id].clips;

        assert_eq!(clips.len(), 2);
        assert_eq!((clips[0].position, clips[0].duration), (40, 7));
        assert_eq!((clips[1].position, clips[1].duration), (47, 5));
        assert_eq!(materialized.arrange_end_samples, 52);
    }

    #[test]
    fn looping_section_is_flattened_into_independent_linear_passes() {
        let mut section = audio_section("Groove A", 1.0, 4);
        section.looping = true;
        let track_id = *section.timeline.by_track.keys().next().unwrap();
        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(0, 8, 120.0);
        capture.start(0, Some((CapturedSectionSource::from_section(&section), 0)));
        let clips = &capture.finish(10).unwrap().materialize().by_track[&track_id].clips;

        assert_eq!(clips.len(), 3);
        assert_eq!(
            clips.iter().map(|clip| clip.position).collect::<Vec<_>>(),
            [0, 4, 8]
        );
        assert_eq!(
            clips.iter().map(|clip| clip.duration).collect::<Vec<_>>(),
            [4, 4, 2]
        );
        assert!(clips.windows(2).all(|pair| pair[0].id != pair[1].id));
    }

    #[test]
    fn midi_sections_keep_effective_transition_alignment_and_groove_identity() {
        let track_id = TrackId::new();
        let first = midi_section("Groove A", track_id, 36);
        let second = midi_section("Groove B", track_id, 38);
        let source_ids = [
            first.timeline.get(track_id).unwrap().note_clips[0].id,
            second.timeline.get(track_id).unwrap().note_clips[0].id,
        ];
        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(8, 8, 120.0);
        capture.start(0, Some((CapturedSectionSource::from_section(&first), 0)));
        capture.transition(CapturedSectionSource::from_section(&second), 5);
        let clips = &capture.finish(9).unwrap().materialize().by_track[&track_id].note_clips;

        assert_eq!(clips.len(), 2);
        assert!((clips[0].position_beats - 2.0).abs() < 1e-9);
        assert!((clips[0].duration_beats - 1.25).abs() < 1e-9);
        assert!((clips[1].position_beats - 3.25).abs() < 1e-9);
        assert_eq!([clips[0].notes[0].pitch, clips[1].notes[0].pitch], [36, 38]);
        assert!(clips
            .iter()
            .all(|clip| clip.groove_grid == GrooveGrid::Sixteenth));
        assert!(clips.iter().all(|clip| !source_ids.contains(&clip.id)));
    }

    #[test]
    fn transition_reported_after_stop_cannot_extend_the_capture() {
        let first = audio_section("Groove A", 4.0, 16);
        let second = audio_section("Groove B", 4.0, 16);
        let track_id = *first.timeline.by_track.keys().next().unwrap();
        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(0, 8, 120.0);
        capture.start(0, Some((CapturedSectionSource::from_section(&first), 0)));
        let completed = capture.finish(4).unwrap();
        capture.transition(CapturedSectionSource::from_section(&second), 6);

        let clips = &completed.materialize().by_track[&track_id].clips;
        assert_eq!(clips.len(), 1);
        assert_eq!((clips[0].position, clips[0].duration), (0, 4));
        assert_eq!(capture.phase, CapturePhase::Idle);
    }

    #[test]
    fn source_edits_after_transition_cannot_rewrite_capture_snapshot() {
        let mut section = audio_section("Breakdown", 4.0, 16);
        let track_id = *section.timeline.by_track.keys().next().unwrap();
        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(20, 8, 120.0);
        capture.start(
            100,
            Some((CapturedSectionSource::from_section(&section), 0)),
        );
        Arc::make_mut(&mut section.timeline)
            .ensure(track_id)
            .clips
            .clear();

        let materialized = capture.finish(108).unwrap().materialize();
        assert_eq!(materialized.by_track[&track_id].clips.len(), 1);
        assert!(section.timeline.get(track_id).unwrap().clips.is_empty());
    }

    #[test]
    fn occupied_target_interval_is_rejected_without_touching_outside_content() {
        let section = audio_section("Drop", 4.0, 16);
        let track_id = *section.timeline.by_track.keys().next().unwrap();
        let mut capture = CaptureState::default();
        capture.phase = CapturePhase::Starting;
        capture.prepare(40, 8, 120.0);
        capture.start(0, Some((CapturedSectionSource::from_section(&section), 0)));
        let completed = capture.finish(8).unwrap();
        let mut arrange = ArrangementTimeline::default();
        let silent_track = TrackId::new();
        let mut outside = section.timeline.get(track_id).unwrap().clips[0].clone();
        outside.position = 44;
        arrange.ensure(silent_track).clips.push(outside);

        let materialized = completed.materialize();
        assert!(materialized.target_interval_is_empty(&ArrangementTimeline::default(), &[track_id]));
        assert!(!materialized.target_interval_is_empty(&arrange, &[track_id, silent_track]));
    }
}
