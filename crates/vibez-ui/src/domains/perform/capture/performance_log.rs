//! Live-note and pointer-automation facts captured from engine-effective events.

use std::collections::HashMap;

use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::TrackId;
use vibez_core::midi::MidiNote;
use vibez_core::perform::{GrooveGrid, NoteRepeatRate};
use vibez_engine::events::AutomationGesturePhase;

use crate::state::{TrackTimelineContent, UiNoteClip};

use super::{samples_per_beat, CaptureClock, MaterializedCapture};

const AUTOMATION_THIN_TOLERANCE: f32 = 0.002;

pub(super) fn append_automation_window(
    destination: &mut TrackTimelineContent,
    source: &TrackTimelineContent,
    window_start_beats: f64,
    window_end_beats: f64,
    destination_start_beats: f64,
) {
    for source_lane in &source.automation {
        let mut mapped = Vec::new();
        if let Some(value) = source_lane.value_at(window_start_beats) {
            mapped.push(AutomationPoint {
                beat: destination_start_beats,
                value,
                curve: 0.0,
            });
        }
        mapped.extend(
            source_lane
                .points
                .iter()
                .filter(|point| point.beat > window_start_beats && point.beat < window_end_beats)
                .map(|point| AutomationPoint {
                    beat: destination_start_beats + point.beat - window_start_beats,
                    ..*point
                }),
        );
        if let Some(value) = source_lane.value_at(window_end_beats) {
            mapped.push(AutomationPoint {
                beat: destination_start_beats + window_end_beats - window_start_beats,
                value,
                curve: 0.0,
            });
        }
        if mapped.is_empty() {
            continue;
        }
        let lane_index = destination
            .automation
            .iter()
            .position(|lane| lane.target == source_lane.target)
            .unwrap_or_else(|| {
                destination
                    .automation
                    .push(AutomationLane::new(source_lane.target));
                destination.automation.len() - 1
            });
        let lane = &mut destination.automation[lane_index];
        for point in mapped {
            lane.insert_point(point);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct OpenNote {
    track_id: TrackId,
    pitch: u8,
    velocity: u8,
    effective_at_samples: u64,
}

#[derive(Debug, Clone, Copy)]
struct CapturedLiveNote {
    track_id: TrackId,
    pitch: u8,
    velocity: u8,
    start_samples: u64,
    end_samples: u64,
    heard_at_samples: u64,
    groove_grid: GrooveGrid,
}

#[derive(Debug, Clone, Copy)]
struct CapturedAutomationEvent {
    track_id: TrackId,
    target: AutomationTarget,
    value: f32,
    phase: AutomationGesturePhase,
    effective_at_samples: u64,
}

#[derive(Debug, Default)]
pub(super) struct PerformanceLog {
    open_notes: Vec<OpenNote>,
    notes: Vec<CapturedLiveNote>,
    automation: Vec<CapturedAutomationEvent>,
}

#[derive(Debug, Default)]
pub(super) struct CompletedPerformanceLog {
    notes: Vec<CapturedLiveNote>,
    automation: Vec<CapturedAutomationEvent>,
}

impl PerformanceLog {
    pub(super) fn input_note(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        on: bool,
        effective_at_samples: u64,
        capture_start_samples: u64,
    ) {
        if effective_at_samples < capture_start_samples {
            return;
        }
        if on {
            self.open_notes.push(OpenNote {
                track_id,
                pitch,
                velocity,
                effective_at_samples,
            });
        } else if let Some(index) = self
            .open_notes
            .iter()
            .rposition(|note| note.track_id == track_id && note.pitch == pitch)
        {
            let note = self.open_notes.remove(index);
            self.notes.push(CapturedLiveNote {
                track_id,
                pitch,
                velocity: note.velocity,
                start_samples: note.effective_at_samples,
                end_samples: effective_at_samples.max(note.effective_at_samples + 1),
                heard_at_samples: note.effective_at_samples,
                groove_grid: GrooveGrid::Off,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn repeated_note(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: NoteRepeatRate,
        effective_at_samples: u64,
        canonical_at_samples: u64,
        capture_start_samples: u64,
        clock: CaptureClock,
    ) {
        if effective_at_samples < capture_start_samples {
            return;
        }
        let duration = (rate.interval_beats() * samples_per_beat(clock.sample_rate, clock.bpm))
            .round()
            .max(1.0) as u64;
        self.notes.push(CapturedLiveNote {
            track_id,
            pitch,
            velocity,
            start_samples: canonical_at_samples,
            end_samples: canonical_at_samples.saturating_add(duration),
            heard_at_samples: effective_at_samples,
            groove_grid: match rate {
                NoteRepeatRate::Eighth => GrooveGrid::Eighth,
                NoteRepeatRate::Sixteenth => GrooveGrid::Sixteenth,
                _ => GrooveGrid::Off,
            },
        });
    }

    pub(super) fn automation_changed(
        &mut self,
        track_id: TrackId,
        target: AutomationTarget,
        value: f32,
        phase: AutomationGesturePhase,
        effective_at_samples: u64,
        capture_start_samples: u64,
    ) {
        if effective_at_samples < capture_start_samples || target == AutomationTarget::TrackMute {
            return;
        }
        self.automation.push(CapturedAutomationEvent {
            track_id,
            target,
            value: value.clamp(0.0, 1.0),
            phase,
            effective_at_samples,
        });
    }

    pub(super) fn finish(mut self, effective_at_samples: u64) -> CompletedPerformanceLog {
        for note in self.open_notes.drain(..) {
            if note.effective_at_samples < effective_at_samples {
                self.notes.push(CapturedLiveNote {
                    track_id: note.track_id,
                    pitch: note.pitch,
                    velocity: note.velocity,
                    start_samples: note.effective_at_samples,
                    end_samples: effective_at_samples,
                    heard_at_samples: note.effective_at_samples,
                    groove_grid: GrooveGrid::Off,
                });
            }
        }
        CompletedPerformanceLog {
            notes: self.notes,
            automation: self.automation,
        }
    }
}

impl CompletedPerformanceLog {
    pub(super) fn materialize(
        &self,
        result: &mut MaterializedCapture,
        clock: CaptureClock,
        engine_start_samples: u64,
        engine_end_samples: u64,
    ) {
        self.materialize_notes(result, clock, engine_start_samples, engine_end_samples);
        self.materialize_automation(result, clock, engine_start_samples, engine_end_samples);
    }

    fn materialize_notes(
        &self,
        result: &mut MaterializedCapture,
        clock: CaptureClock,
        engine_start_samples: u64,
        engine_end_samples: u64,
    ) {
        let spb = result.samples_per_beat;
        let clip_position_beats = result.arrange_start_samples as f64 / spb;
        let clip_duration_beats = result
            .arrange_end_samples
            .saturating_sub(result.arrange_start_samples) as f64
            / spb;
        let mut grouped: HashMap<(TrackId, GrooveGrid), Vec<MidiNote>> = HashMap::new();
        for note in &self.notes {
            if note.heard_at_samples < engine_start_samples
                || note.heard_at_samples >= engine_end_samples
            {
                continue;
            }
            let start = note.start_samples.max(engine_start_samples);
            let end = note.end_samples.min(engine_end_samples);
            if end <= start {
                continue;
            }
            grouped
                .entry((note.track_id, note.groove_grid))
                .or_default()
                .push(MidiNote {
                    pitch: note.pitch,
                    velocity: note.velocity,
                    start_beat: start.saturating_sub(engine_start_samples) as f64 / spb,
                    duration_beats: (end - start) as f64 / spb,
                });
        }
        for ((track_id, groove_grid), mut notes) in grouped {
            notes.sort_by(|left, right| {
                left.start_beat
                    .total_cmp(&right.start_beat)
                    .then(left.pitch.cmp(&right.pitch))
            });
            result
                .by_track
                .entry(track_id)
                .or_default()
                .note_clips
                .push(UiNoteClip {
                    id: vibez_core::id::ClipId::new(),
                    name: match groove_grid {
                        GrooveGrid::Off => "Capture · Live notes",
                        GrooveGrid::Eighth => "Capture · Note Repeat · 1/8",
                        GrooveGrid::Sixteenth => "Capture · Note Repeat · 1/16",
                    }
                    .into(),
                    position_beats: clip_position_beats,
                    duration_beats: clip_duration_beats,
                    notes,
                    selected_notes: Default::default(),
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                    groove_grid,
                });
        }
        let _ = clock;
    }

    fn materialize_automation(
        &self,
        result: &mut MaterializedCapture,
        _clock: CaptureClock,
        engine_start_samples: u64,
        engine_end_samples: u64,
    ) {
        let mut grouped: HashMap<(TrackId, AutomationTarget), Vec<CapturedAutomationEvent>> =
            HashMap::new();
        for event in &self.automation {
            if event.effective_at_samples >= engine_start_samples
                && event.effective_at_samples <= engine_end_samples
            {
                grouped
                    .entry((event.track_id, event.target))
                    .or_default()
                    .push(*event);
            }
        }
        for ((track_id, target), mut events) in grouped {
            events.sort_by_key(|event| event.effective_at_samples);
            let content = result.by_track.entry(track_id).or_default();
            let lane_index = content
                .automation
                .iter()
                .position(|lane| lane.target == target)
                .unwrap_or_else(|| {
                    content.automation.push(AutomationLane::new(target));
                    content.automation.len() - 1
                });
            apply_gestures_to_lane(
                &mut content.automation[lane_index],
                &events,
                result.arrange_start_samples,
                result.arrange_end_samples,
                engine_start_samples,
                engine_end_samples,
                result.samples_per_beat,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_gestures_to_lane(
    lane: &mut AutomationLane,
    events: &[CapturedAutomationEvent],
    arrange_start_samples: u64,
    arrange_end_samples: u64,
    engine_start_samples: u64,
    engine_end_samples: u64,
    samples_per_beat: f64,
) {
    let to_beat = |effective: u64| {
        arrange_start_samples.saturating_add(effective.saturating_sub(engine_start_samples)) as f64
            / samples_per_beat
    };
    let capture_start_beat = arrange_start_samples as f64 / samples_per_beat;
    let capture_end_beat = arrange_end_samples as f64 / samples_per_beat;
    let epsilon = 1.0 / samples_per_beat;
    let mut index = 0;
    while index < events.len() {
        while index < events.len() && events[index].phase != AutomationGesturePhase::Begin {
            index += 1;
        }
        if index == events.len() {
            break;
        }
        let start_index = index;
        index += 1;
        while index < events.len()
            && events[index].phase != AutomationGesturePhase::End
            && events[index].phase != AutomationGesturePhase::Begin
        {
            index += 1;
        }
        let end_event = (index < events.len()
            && events[index].phase == AutomationGesturePhase::End)
            .then_some(events[index]);
        let end_index = end_event.map_or(index, |_| index + 1);
        let start_beat = to_beat(events[start_index].effective_at_samples);
        let end_beat = end_event
            .map(|event| to_beat(event.effective_at_samples))
            .unwrap_or(capture_end_beat);
        let baseline_before = lane
            .value_at((start_beat - epsilon).max(capture_start_beat))
            .unwrap_or(events[start_index].value);
        let baseline_after = end_event
            .map(|event| event.value)
            .or_else(|| lane.value_at(end_beat))
            .unwrap_or(events[end_index.saturating_sub(1)].value);
        lane.points
            .retain(|point| point.beat < start_beat || point.beat >= end_beat);
        lane.insert_point(AutomationPoint {
            beat: (start_beat - epsilon).max(capture_start_beat),
            value: baseline_before,
            curve: 0.0,
        });
        let gesture_end = end_event.map_or(index, |_| index);
        let raw: Vec<_> = events[start_index..gesture_end]
            .iter()
            .map(|event| (to_beat(event.effective_at_samples), event.value))
            .collect();
        for (beat, value) in thin_points(&raw, AUTOMATION_THIN_TOLERANCE) {
            lane.insert_point(AutomationPoint {
                beat,
                value,
                curve: 0.0,
            });
        }
        if let Some(last) = raw.last() {
            lane.insert_point(AutomationPoint {
                beat: (end_beat - epsilon).max(start_beat),
                value: last.1,
                curve: 0.0,
            });
        }
        lane.insert_point(AutomationPoint {
            beat: end_beat.min(capture_end_beat),
            value: baseline_after,
            curve: 0.0,
        });
        index = end_index;
        if end_beat >= to_beat(engine_end_samples) {
            break;
        }
    }
}

fn thin_points(points: &[(f64, f32)], tolerance: f32) -> Vec<(f64, f32)> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    thin_range(points, tolerance, 0, points.len() - 1, &mut keep);
    points
        .iter()
        .copied()
        .zip(keep)
        .filter_map(|(point, keep)| keep.then_some(point))
        .collect()
}

fn thin_range(points: &[(f64, f32)], tolerance: f32, start: usize, end: usize, keep: &mut [bool]) {
    if end <= start + 1 {
        return;
    }
    let (start_x, start_y) = points[start];
    let (end_x, end_y) = points[end];
    let span = (end_x - start_x).max(f64::EPSILON);
    let mut worst = (0.0f32, start);
    for (offset, (x, y)) in points[start + 1..end].iter().copied().enumerate() {
        let t = ((x - start_x) / span) as f32;
        let error = (y - (start_y + (end_y - start_y) * t)).abs();
        if error > worst.0 {
            worst = (error, start + 1 + offset);
        }
    }
    if worst.0 > tolerance {
        keep[worst.1] = true;
        thin_range(points, tolerance, start, worst.1, keep);
        thin_range(points, tolerance, worst.1, end, keep);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn materialized() -> MaterializedCapture {
        MaterializedCapture {
            arrange_start_samples: 200,
            arrange_end_samples: 600,
            samples_per_beat: 100.0,
            ..MaterializedCapture::default()
        }
    }

    #[test]
    fn free_repeat_and_stop_truncated_notes_keep_their_audible_facts() {
        let track_id = TrackId::new();
        let clock = CaptureClock {
            arrange_start_samples: 200,
            sample_rate: 100,
            bpm: 60.0,
        };
        let mut log = PerformanceLog::default();
        // This onset preceded Capture, so its later release must not create a note.
        log.input_note(track_id, 30, 90, true, 999, 1_000);
        log.input_note(track_id, 30, 0, false, 1_050, 1_000);
        log.input_note(track_id, 36, 101, true, 1_050, 1_000);
        log.input_note(track_id, 36, 0, false, 1_150, 1_000);
        log.repeated_note(
            track_id,
            36,
            88,
            NoteRepeatRate::Sixteenth,
            1_200,
            1_180,
            1_000,
            clock,
        );
        log.input_note(track_id, 40, 77, true, 1_300, 1_000);

        let completed = log.finish(1_400);
        let mut result = materialized();
        completed.materialize(&mut result, clock, 1_000, 1_400);

        let clips = &result.by_track[&track_id].note_clips;
        assert_eq!(clips.len(), 2);
        let free = clips
            .iter()
            .find(|clip| clip.groove_grid == GrooveGrid::Off)
            .unwrap();
        assert_eq!(free.notes.len(), 2);
        assert_eq!(
            (
                free.notes[0].pitch,
                free.notes[0].velocity,
                free.notes[0].start_beat,
                free.notes[0].duration_beats,
            ),
            (36, 101, 0.5, 1.0)
        );
        assert_eq!(
            (
                free.notes[1].pitch,
                free.notes[1].start_beat,
                free.notes[1].duration_beats,
            ),
            (40, 3.0, 1.0)
        );
        let repeated = clips
            .iter()
            .find(|clip| clip.groove_grid == GrooveGrid::Sixteenth)
            .unwrap();
        assert_eq!(repeated.notes[0].pitch, 36);
        assert_eq!(repeated.notes[0].velocity, 88);
        assert!((repeated.notes[0].start_beat - 1.8).abs() < 1e-6);
        assert!((repeated.notes[0].duration_beats - 0.25).abs() < 1e-6);
    }

    #[test]
    fn copied_baseline_keeps_edges_and_internal_shape() {
        let mut source = TrackTimelineContent::default();
        let mut lane = AutomationLane::new(AutomationTarget::TrackGain);
        for (beat, value) in [(0.0, 0.1), (2.0, 0.5), (4.0, 0.9)] {
            lane.insert_point(AutomationPoint {
                beat,
                value,
                curve: 0.0,
            });
        }
        source.automation.push(lane);
        let mut destination = TrackTimelineContent::default();

        append_automation_window(&mut destination, &source, 1.0, 3.0, 8.0);

        let lane = &destination.automation[0];
        assert!((lane.value_at(8.0).unwrap() - 0.3).abs() < 1e-6);
        assert!((lane.value_at(9.0).unwrap() - 0.5).abs() < 1e-6);
        assert!((lane.value_at(10.0).unwrap() - 0.7).abs() < 1e-6);
    }

    #[test]
    fn linear_fit_thinning_keeps_shape_changes_and_drops_redundant_points() {
        let thinned = thin_points(&[(0.0, 0.0), (1.0, 0.25), (2.0, 0.5), (3.0, 0.9)], 0.01);
        assert_eq!(thinned, [(0.0, 0.0), (2.0, 0.5), (3.0, 0.9)]);
    }

    #[test]
    fn gesture_splice_pins_baseline_and_yields_back() {
        let track_id = TrackId::new();
        let events = [
            CapturedAutomationEvent {
                track_id,
                target: AutomationTarget::TrackPan,
                value: 0.8,
                phase: AutomationGesturePhase::Begin,
                effective_at_samples: 200,
            },
            CapturedAutomationEvent {
                track_id,
                target: AutomationTarget::TrackPan,
                value: 0.9,
                phase: AutomationGesturePhase::Update,
                effective_at_samples: 250,
            },
            CapturedAutomationEvent {
                track_id,
                target: AutomationTarget::TrackPan,
                value: 0.25,
                phase: AutomationGesturePhase::End,
                effective_at_samples: 300,
            },
        ];
        let mut lane = AutomationLane::new(AutomationTarget::TrackPan);
        lane.insert_point(AutomationPoint {
            beat: 0.0,
            value: 0.25,
            curve: 0.0,
        });
        lane.insert_point(AutomationPoint {
            beat: 10.0,
            value: 0.25,
            curve: 0.0,
        });

        apply_gestures_to_lane(&mut lane, &events, 0, 1_000, 0, 1_000, 100.0);

        assert!((lane.value_at(1.99).unwrap() - 0.25).abs() < 1e-6);
        assert!((lane.value_at(2.0).unwrap() - 0.8).abs() < 1e-6);
        assert!((lane.value_at(2.5).unwrap() - 0.9).abs() < 1e-6);
        assert!((lane.value_at(3.0).unwrap() - 0.25).abs() < 1e-6);
    }
}
