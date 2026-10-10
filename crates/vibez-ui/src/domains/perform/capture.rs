//! Runtime Capture log and pure Section-to-Arrange materialization.
//!
//! The audio engine owns effective timestamps. This module snapshots the
//! canonical Section source at those boundaries, then creates independent
//! linear Arrange clips only after the engine confirms Capture stop.

use std::collections::HashMap;
use std::sync::Arc;

use vibez_core::automation::{AutomationLane, AutomationPoint, AutomationTarget};
use vibez_core::id::{ClipId, TrackId};
use vibez_core::midi::MidiNote;

use crate::state::{ArrangementTimeline, TrackTimelineContent, UiClip, UiNoteClip, UndoGestureId};

use super::{PerformAction, Section};

mod performance_log;
use performance_log::{CompletedPerformanceLog, PerformanceLog};

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
pub struct CapturedTimelineSource {
    pub name: String,
    pub length_beats: f64,
    pub looping: bool,
    pub timeline: Arc<ArrangementTimeline>,
    pub direct_leads: Arc<[(TrackId, u32)]>,
}

impl CapturedTimelineSource {
    pub fn from_clip(clip: &super::LauncherClip, samples_per_beat: f64) -> Self {
        let (length, looping) = clip.length_and_loop(samples_per_beat);
        Self {
            name: clip.name().into(),
            length_beats: length as f64 / samples_per_beat,
            looping,
            timeline: Arc::clone(&clip.timeline),
            direct_leads: Arc::from([]),
        }
    }

    pub fn from_section(section: &Section) -> Self {
        Self {
            name: section.name.clone(),
            length_beats: section.length_beats,
            looping: section.looping,
            timeline: Arc::clone(&section.timeline),
            direct_leads: Arc::from([]),
        }
    }

    pub fn from_section_with_offsets(section: &Section, offsets: Arc<[(TrackId, u32)]>) -> Self {
        let mut source = Self::from_section(section);
        source.direct_leads = offsets;
        source
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
    source: CapturedTimelineSource,
    effective_start_samples: u64,
    source_start_samples: u64,
}

#[derive(Debug, Clone)]
struct CapturedTimelineSpan {
    source: CapturedTimelineSource,
    end_follows_source: bool,
    effective_start_samples: u64,
    effective_end_samples: u64,
    source_start_samples: u64,
}

#[derive(Debug)]
struct CaptureSession {
    clock: CaptureClock,
    engine_start_samples: u64,
    active: Option<ActiveSpan>,
    active_clips: HashMap<TrackId, ActiveSpan>,
    spans: Vec<CapturedTimelineSpan>,
    controlled_tracks: Vec<(TrackId, bool)>,
    mute_changes: Vec<CapturedMuteChange>,
    performance: PerformanceLog,
}

#[derive(Debug)]
pub struct CompletedCapture {
    clock: CaptureClock,
    engine_start_samples: u64,
    engine_end_samples: u64,
    spans: Vec<CapturedTimelineSpan>,
    controlled_tracks: Vec<(TrackId, bool)>,
    mute_changes: Vec<CapturedMuteChange>,
    performance: CompletedPerformanceLog,
}

#[derive(Debug, Clone, Copy)]
struct CapturedMuteChange {
    track_id: TrackId,
    muted: bool,
    effective_at_samples: u64,
}

#[derive(Debug, Default)]
pub struct MaterializedCapture {
    pub arrange_start_samples: u64,
    pub arrange_end_samples: u64,
    pub by_track: HashMap<TrackId, TrackTimelineContent>,
    /// Every Project Track controlled by Perform, including tracks whose
    /// recorded performance was silence.
    pub controlled_track_ids: Vec<TrackId>,
    /// Manual mute state heard at Capture start, used to close mute lanes and
    /// return the mixer to its pre-take state after commit.
    pub pre_capture_mutes: HashMap<TrackId, bool>,
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
            controlled_track_ids: Vec::new(),
            pre_capture_mutes: HashMap::new(),
            samples_per_beat: samples_per_beat(self.clock.sample_rate, self.clock.bpm),
        };
        let samples_per_beat = result.samples_per_beat;
        if samples_per_beat <= 0.0 {
            return result;
        }

        for span in &self.spans {
            let section_length = (span.source.length_beats * samples_per_beat)
                .round()
                .max(1.0) as u64;
            for (track_id, content) in &span.source.timeline.by_track {
                let lead = span
                    .source
                    .direct_leads
                    .iter()
                    .find(|(id, _)| id == track_id)
                    .map_or(0, |(_, lead)| *lead);
                let start = i128::from(span.effective_start_samples) - i128::from(lead);
                // A source change moves its audible boundary. Ending the take
                // only clips the source that is still sounding at that moment.
                let end = if span.end_follows_source {
                    i128::from(span.effective_end_samples) - i128::from(lead)
                } else {
                    i128::from(self.engine_end_samples)
                };
                let kept_start = start.max(i128::from(self.engine_start_samples));
                let kept_end = end.min(i128::from(self.engine_end_samples));
                if kept_end <= kept_start {
                    continue;
                }
                let source_start = span
                    .source_start_samples
                    .saturating_add((kept_start - start) as u64);
                let mut source_cursor = if span.source.looping {
                    source_start % section_length
                } else {
                    source_start
                };
                let mut remaining = (kept_end - kept_start) as u64;
                let mut destination_cursor = self
                    .clock
                    .arrange_start_samples
                    .saturating_add((kept_start - i128::from(self.engine_start_samples)) as u64);
                while remaining > 0 && source_cursor < section_length {
                    let segment_length = remaining.min(section_length - source_cursor);
                    append_track_window(
                        result.by_track.entry(*track_id).or_default(),
                        content,
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
        }

        for (track_id, pre_capture_muted) in &self.controlled_tracks {
            let changes: Vec<_> = self
                .mute_changes
                .iter()
                .filter(|change| change.track_id == *track_id)
                .collect();
            if changes.is_empty() {
                continue;
            }
            let mut lane = AutomationLane::new(AutomationTarget::TrackMute);
            let start_beat = result.arrange_start_samples as f64 / samples_per_beat;
            lane.insert_point(AutomationPoint {
                beat: start_beat,
                value: if *pre_capture_muted { 1.0 } else { 0.0 },
                curve: 0.0,
            });
            for change in changes {
                if change.effective_at_samples < self.engine_start_samples
                    || change.effective_at_samples > self.engine_end_samples
                {
                    continue;
                }
                let arrange_samples = self.clock.arrange_start_samples.saturating_add(
                    change
                        .effective_at_samples
                        .saturating_sub(self.engine_start_samples),
                );
                lane.insert_point(AutomationPoint {
                    beat: arrange_samples as f64 / samples_per_beat,
                    value: if change.muted { 1.0 } else { 0.0 },
                    curve: 0.0,
                });
            }
            lane.insert_point(AutomationPoint {
                beat: result.arrange_end_samples as f64 / samples_per_beat,
                value: if *pre_capture_muted { 1.0 } else { 0.0 },
                curve: 0.0,
            });
            result
                .by_track
                .entry(*track_id)
                .or_default()
                .automation
                .push(lane);
        }

        self.performance.materialize(
            &mut result,
            self.clock,
            self.engine_start_samples,
            self.engine_end_samples,
        );
        for content in result.by_track.values_mut() {
            content.clips.sort_by_key(|clip| clip.position);
            content
                .note_clips
                .sort_by(|left, right| left.position_beats.total_cmp(&right.position_beats));
        }
        result.controlled_track_ids = self
            .controlled_tracks
            .iter()
            .map(|(track_id, _)| *track_id)
            .collect();
        result.pre_capture_mutes = self.controlled_tracks.iter().copied().collect();
        result
    }
}

#[derive(Debug, Default)]
pub struct CaptureState {
    pub phase: CapturePhase,
    prepared_clock: Option<CaptureClock>,
    session: Option<CaptureSession>,
    prepared_controlled_tracks: Vec<(TrackId, bool)>,
    active_automation_gesture: Option<UndoGestureId>,
    active_automation_targets: Vec<(TrackId, AutomationTarget)>,
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

    pub fn is_controlled_track(&self, track_id: TrackId) -> bool {
        self.session.as_ref().is_some_and(|session| {
            session
                .controlled_tracks
                .iter()
                .any(|(controlled_id, _)| *controlled_id == track_id)
        })
    }

    pub fn begin_ui_automation_gesture(
        &mut self,
        gesture: UndoGestureId,
    ) -> Vec<(TrackId, AutomationTarget)> {
        if self.active_automation_gesture == Some(gesture) {
            return Vec::new();
        }
        let ended = self.end_ui_automation_gesture();
        self.active_automation_gesture = Some(gesture);
        ended
    }

    pub fn register_ui_automation_target(
        &mut self,
        track_id: TrackId,
        target: AutomationTarget,
    ) -> bool {
        let target = (track_id, target);
        if self.active_automation_targets.contains(&target) {
            false
        } else {
            self.active_automation_targets.push(target);
            true
        }
    }

    pub fn end_ui_automation_gesture(&mut self) -> Vec<(TrackId, AutomationTarget)> {
        self.active_automation_gesture = None;
        std::mem::take(&mut self.active_automation_targets)
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

    pub fn prepare_controlled_tracks(&mut self, tracks: impl IntoIterator<Item = (TrackId, bool)>) {
        self.prepared_controlled_tracks = tracks.into_iter().collect();
    }

    pub fn start(
        &mut self,
        effective_at_samples: u64,
        active: Option<(CapturedTimelineSource, u64)>,
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
            active_clips: HashMap::new(),
            controlled_tracks: std::mem::take(&mut self.prepared_controlled_tracks),
            mute_changes: Vec::new(),
            performance: PerformanceLog::default(),
        });
        self.phase = CapturePhase::Recording;
    }

    pub fn track_mute_changed(
        &mut self,
        track_id: TrackId,
        muted: bool,
        effective_at_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !matches!(self.phase, CapturePhase::Recording | CapturePhase::Stopping)
            || effective_at_samples < session.engine_start_samples
            || !session
                .controlled_tracks
                .iter()
                .any(|(controlled_id, _)| *controlled_id == track_id)
        {
            return;
        }
        session.mute_changes.push(CapturedMuteChange {
            track_id,
            muted,
            effective_at_samples,
        });
    }

    pub fn input_note(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        on: bool,
        effective_at_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !matches!(self.phase, CapturePhase::Recording | CapturePhase::Stopping)
            || !session
                .controlled_tracks
                .iter()
                .any(|(controlled_id, _)| *controlled_id == track_id)
        {
            return;
        }
        session.performance.input_note(
            track_id,
            pitch,
            velocity,
            on,
            effective_at_samples,
            session.engine_start_samples,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn repeated_note(
        &mut self,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: vibez_core::perform::NoteRepeatRate,
        effective_at_samples: u64,
        canonical_at_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !matches!(self.phase, CapturePhase::Recording | CapturePhase::Stopping)
            || !session
                .controlled_tracks
                .iter()
                .any(|(controlled_id, _)| *controlled_id == track_id)
        {
            return;
        }
        session.performance.repeated_note(
            track_id,
            pitch,
            velocity,
            rate,
            effective_at_samples,
            canonical_at_samples,
            session.engine_start_samples,
            session.clock,
        );
    }

    pub fn automation_changed(
        &mut self,
        track_id: TrackId,
        target: AutomationTarget,
        value: f32,
        phase: vibez_engine::events::AutomationGesturePhase,
        effective_at_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !matches!(self.phase, CapturePhase::Recording | CapturePhase::Stopping)
            || !session
                .controlled_tracks
                .iter()
                .any(|(controlled_id, _)| *controlled_id == track_id)
        {
            return;
        }
        session.performance.automation_changed(
            track_id,
            target,
            value,
            phase,
            effective_at_samples,
            session.engine_start_samples,
        );
    }

    pub fn transition(&mut self, source: CapturedTimelineSource, effective_at_samples: u64) {
        self.refresh(source, effective_at_samples, 0);
    }

    pub fn refresh(
        &mut self,
        source: CapturedTimelineSource,
        effective_at_samples: u64,
        source_start_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        close_active_span(session, effective_at_samples, true);
        session.active = Some(ActiveSpan {
            source,
            effective_start_samples: effective_at_samples,
            source_start_samples,
        });
    }

    pub fn clip_transition(
        &mut self,
        track_id: TrackId,
        source: Option<CapturedTimelineSource>,
        effective_at_samples: u64,
        source_start_samples: u64,
    ) {
        let Some(session) = &mut self.session else {
            return;
        };
        if let Some(active) = session.active_clips.remove(&track_id) {
            close_span(&mut session.spans, active, effective_at_samples, true);
        }
        if let Some(source) = source {
            session.active_clips.insert(
                track_id,
                ActiveSpan {
                    source,
                    effective_start_samples: effective_at_samples,
                    source_start_samples,
                },
            );
        }
    }

    pub fn end_source(&mut self, effective_at_samples: u64) {
        if let Some(session) = &mut self.session {
            close_active_span(session, effective_at_samples, true);
        }
    }

    pub fn finish(&mut self, effective_at_samples: u64) -> Option<CompletedCapture> {
        let Some(mut session) = self.session.take() else {
            self.cancel();
            return None;
        };
        close_active_span(&mut session, effective_at_samples, false);
        for (_, active) in session.active_clips.drain() {
            close_span(&mut session.spans, active, effective_at_samples, false);
        }
        let performance = session.performance.finish(effective_at_samples);
        self.phase = CapturePhase::Idle;
        self.prepared_clock = None;
        self.active_automation_gesture = None;
        self.active_automation_targets.clear();
        Some(CompletedCapture {
            clock: session.clock,
            engine_start_samples: session.engine_start_samples,
            engine_end_samples: effective_at_samples,
            spans: session.spans,
            controlled_tracks: session.controlled_tracks,
            mute_changes: session.mute_changes,
            performance,
        })
    }

    pub fn cancel(&mut self) {
        self.phase = CapturePhase::Idle;
        self.prepared_clock = None;
        self.session = None;
        self.prepared_controlled_tracks.clear();
        self.active_automation_gesture = None;
        self.active_automation_targets.clear();
    }
}

fn close_active_span(
    session: &mut CaptureSession,
    effective_end_samples: u64,
    end_follows_source: bool,
) {
    let Some(active) = session.active.take() else {
        return;
    };
    close_span(
        &mut session.spans,
        active,
        effective_end_samples,
        end_follows_source,
    );
}

fn close_span(
    spans: &mut Vec<CapturedTimelineSpan>,
    active: ActiveSpan,
    effective_end_samples: u64,
    end_follows_source: bool,
) {
    if effective_end_samples > active.effective_start_samples
        || (!end_follows_source && active.source.direct_leads.iter().any(|(_, lead)| *lead > 0))
    {
        spans.push(CapturedTimelineSpan {
            source: active.source,
            end_follows_source,
            effective_start_samples: active.effective_start_samples,
            effective_end_samples,
            source_start_samples: active.source_start_samples,
        });
    }
}

fn samples_per_beat(sample_rate: u32, bpm: f64) -> f64 {
    if bpm > 0.0 {
        vibez_core::time::TempoMap::new(bpm, sample_rate).samples_per_beat()
    } else {
        0.0
    }
}

fn append_track_window(
    destination: &mut TrackTimelineContent,
    content: &TrackTimelineContent,
    source: &CapturedTimelineSource,
    window_start_samples: u64,
    window_end_samples: u64,
    destination_start_samples: u64,
    samples_per_beat: f64,
) {
    let window_start_beats = window_start_samples as f64 / samples_per_beat;
    let window_end_beats = window_end_samples as f64 / samples_per_beat;
    let destination_start_beats = destination_start_samples as f64 / samples_per_beat;

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
        let fragment_duration = overlap_end - overlap_start;
        let (fragment_start, fragment_start_marker, fragment_warp_markers) =
            clip.warp_geometry_for_fragment(delta, fragment_duration);
        let mut fragment = UiClip {
            id: ClipId::new(),
            name: if source.name == clip.name {
                format!("Capture · {}", clip.name)
            } else {
                format!("Capture · {} · {}", source.name, clip.name)
            },
            audio: Arc::clone(&clip.audio),
            source: clip.source.clone(),
            position: destination_start_samples + (overlap_start - window_start_samples),
            source_offset: fragment_start,
            start_marker: fragment_start_marker,
            duration: fragment_duration,
            loop_enabled: clip.loop_enabled,
            loop_start: clip.loop_start,
            loop_end: clip.loop_end,
            gain_db: clip.gain_db,
            fades: clip.fades,
            playback_direction: clip.playback_direction,
            transient_markers: clip.transient_markers.clone(),
            warp_markers: fragment_warp_markers,
            transpose: clip.transpose,
            original_bpm: clip.original_bpm,
            warped: clip.warped,
            warped_to_bpm: clip.warped_to_bpm,
            original_audio: clip.original_audio.as_ref().map(Arc::clone),
        };
        fragment.fades = clip
            .fades
            .for_fragment(clip.duration, delta, fragment.duration);
        fragment
            .transient_markers
            .retain_source_range(fragment.source_offset, fragment.source_end());
        if fragment.loop_enabled && fragment.loop_end <= fragment.loop_start {
            fragment.loop_enabled = false;
        }
        destination.clips.push(fragment);
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
        let notes = captured_visible_notes(clip, local_start, local_end);
        let fragment = UiNoteClip {
            id: ClipId::new(),
            name: if source.name == clip.name {
                format!("Capture · {}", clip.name)
            } else {
                format!("Capture · {} · {}", source.name, clip.name)
            },
            position_beats: destination_start_beats + (overlap_start - window_start_beats),
            duration_beats: overlap_end - overlap_start,
            notes,
            selected_notes: Default::default(),
            start_marker_beats: 0.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: clip.groove_grid,
        };
        destination.note_clips.push(fragment);
    }
    performance_log::append_automation_window(
        destination,
        content,
        window_start_beats,
        window_end_beats,
        destination_start_beats,
    );
}

pub(crate) fn captured_audio_offset(clip: &UiClip, timeline_delta: u64) -> u64 {
    clip.source_frame_at(timeline_delta)
}

pub(crate) fn captured_visible_notes(
    clip: &UiNoteClip,
    local_start: f64,
    local_end: f64,
) -> Vec<MidiNote> {
    let mut visible = Vec::new();
    for note in &clip.notes {
        for occurrence in clip.note_occurrences(note.start_beat) {
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
#[path = "capture_clip_tests.rs"]
mod clip_capture_tests;
#[cfg(test)]
#[path = "capture_compensation_tests.rs"]
mod compensation_tests;
#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "capture_replay_tests.rs"]
mod replay_tests;
