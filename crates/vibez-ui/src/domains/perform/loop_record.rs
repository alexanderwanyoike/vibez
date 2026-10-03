//! Shared loop recording owns note timing, quantization and pass boundaries.

use vibez_core::id::{ClipId, SectionId, TrackId};
#[cfg(test)]
use vibez_core::perform::SwingAmount;
use vibez_core::perform::{GrooveGrid, NoteRepeatRate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopRecordCountIn {
    Off,
    #[default]
    OneBar,
    TwoBars,
}

impl LoopRecordCountIn {
    pub const ALL: [Self; 3] = [Self::Off, Self::OneBar, Self::TwoBars];

    pub const fn bars(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::OneBar => 1,
            Self::TwoBars => 2,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Count-in Off",
            Self::OneBar => "Count-in 1 Bar",
            Self::TwoBars => "Count-in 2 Bars",
        }
    }
}

impl std::fmt::Display for LoopRecordCountIn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopRecordMode {
    #[default]
    Overdub,
    Replace,
}

impl LoopRecordMode {
    pub const ALL: [Self; 2] = [Self::Overdub, Self::Replace];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overdub => "Overdub",
            Self::Replace => "Replace",
        }
    }
}

impl std::fmt::Display for LoopRecordMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopRecordQuantization {
    Off,
    Quarter,
    QuarterTriplet,
    Eighth,
    EighthTriplet,
    #[default]
    Sixteenth,
    SixteenthTriplet,
    ThirtySecond,
    ThirtySecondTriplet,
}

impl LoopRecordQuantization {
    pub const ALL: [Self; 9] = [
        Self::Off,
        Self::Quarter,
        Self::QuarterTriplet,
        Self::Eighth,
        Self::EighthTriplet,
        Self::Sixteenth,
        Self::SixteenthTriplet,
        Self::ThirtySecond,
        Self::ThirtySecondTriplet,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Quantize Off",
            Self::Quarter => "1/4",
            Self::QuarterTriplet => "1/4T",
            Self::Eighth => "1/8",
            Self::EighthTriplet => "1/8T",
            Self::Sixteenth => "1/16",
            Self::SixteenthTriplet => "1/16T",
            Self::ThirtySecond => "1/32",
            Self::ThirtySecondTriplet => "1/32T",
        }
    }

    const fn interval_beats(self) -> Option<f64> {
        match self {
            Self::Off => None,
            Self::Quarter => Some(1.0),
            Self::QuarterTriplet => Some(2.0 / 3.0),
            Self::Eighth => Some(0.5),
            Self::EighthTriplet => Some(1.0 / 3.0),
            Self::Sixteenth => Some(0.25),
            Self::SixteenthTriplet => Some(1.0 / 6.0),
            Self::ThirtySecond => Some(0.125),
            Self::ThirtySecondTriplet => Some(1.0 / 12.0),
        }
    }

    const fn groove_grid(self) -> GrooveGrid {
        match self {
            Self::Eighth => GrooveGrid::Eighth,
            Self::Sixteenth => GrooveGrid::Sixteenth,
            _ => GrooveGrid::Off,
        }
    }
}

impl std::fmt::Display for LoopRecordQuantization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LoopRecordPhase {
    #[default]
    Idle,
    Preparing,
    Armed,
    Recording,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoopRecordMsg {
    Toggle,
    SetCountIn(LoopRecordCountIn),
    SetMode(LoopRecordMode),
    SetQuantization(LoopRecordQuantization),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopRecordStartRequest<T = SectionId> {
    pub target_id: T,
    pub track_id: TrackId,
    pub from_stopped: bool,
    pub count_in_bars: u8,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedLoopNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub groove_grid: GrooveGrid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LoopRecordInput<T = SectionId> {
    pub target_id: Option<T>,
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
    pub effective_at_samples: u64,
    pub local_position_samples: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletedLoopRecording<T = SectionId> {
    pub target_id: T,
    pub track_id: TrackId,
    pub notes: Vec<RecordedLoopNote>,
    pub replace_ranges: Vec<(f64, f64)>,
}

/// Paint-only snapshot of an active take for loop construction.
/// It never enters project state or history; Stop remains the mutation boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct LoopRecordPreview<T = SectionId> {
    pub clip_id: ClipId,
    pub target_id: T,
    pub track_id: TrackId,
    pub position_beats: f64,
    pub length_beats: f64,
    pub notes: Vec<RecordedLoopNote>,
}

#[derive(Debug, Clone, Copy)]
struct OpenNote {
    pitch: u8,
    velocity: u8,
    effective_at_samples: u64,
    local_position_samples: u64,
}

#[derive(Debug, Clone)]
struct RecordingSession<T = SectionId> {
    preview_clip_id: ClipId,
    target_id: T,
    track_id: TrackId,
    bpm: f64,
    sample_rate: u32,
    length_beats: f64,
    length_samples: u64,
    mode: LoopRecordMode,
    quantization: LoopRecordQuantization,
    started_at_samples: Option<u64>,
    started_at_local_samples: u64,
    last_local_samples: u64,
    preview_wrapped: bool,
    replace_wrapped: bool,
    open_notes: Vec<OpenNote>,
    notes: Vec<RecordedLoopNote>,
}

#[derive(Debug, Clone)]
pub struct LoopRecordState<T = SectionId> {
    pub count_in: LoopRecordCountIn,
    pub mode: LoopRecordMode,
    pub quantization: LoopRecordQuantization,
    pub phase: LoopRecordPhase,
    pub pending_boundary_samples: Option<u64>,
    arm_sent: bool,
    pub(super) transport_playing: bool,
    bpm: f64,
    sample_rate: u32,
    session: Option<RecordingSession<T>>,
}

impl<T> Default for LoopRecordState<T> {
    fn default() -> Self {
        Self {
            count_in: LoopRecordCountIn::default(),
            mode: LoopRecordMode::default(),
            quantization: LoopRecordQuantization::default(),
            phase: LoopRecordPhase::Idle,
            pending_boundary_samples: None,
            arm_sent: false,
            transport_playing: false,
            bpm: 120.0,
            sample_rate: 44_100,
            session: None,
        }
    }
}

impl<T: Copy + PartialEq> LoopRecordState<T> {
    pub fn sync_clock(&mut self, transport_playing: bool, bpm: f64, sample_rate: u32) {
        self.transport_playing = transport_playing;
        self.bpm = bpm;
        self.sample_rate = sample_rate;
    }

    pub fn is_active(&self) -> bool {
        self.phase != LoopRecordPhase::Idle
    }

    pub fn target(&self) -> Option<(T, TrackId)> {
        self.session
            .as_ref()
            .map(|session| (session.target_id, session.track_id))
    }

    pub fn live_preview(&self) -> Option<LoopRecordPreview<T>> {
        let session = self.session.as_ref().filter(|session| {
            session.started_at_samples.is_some()
                && matches!(
                    self.phase,
                    LoopRecordPhase::Recording | LoopRecordPhase::Stopping
                )
        })?;
        let mut notes = session.notes.clone();
        notes.extend(
            session
                .open_notes
                .iter()
                .map(|note| preview_open_note(session, *note)),
        );
        notes.sort_by(|left, right| {
            left.start_beat
                .total_cmp(&right.start_beat)
                .then(left.pitch.cmp(&right.pitch))
        });
        let (position_beats, length_beats) = if session.preview_wrapped {
            (0.0, session.length_beats)
        } else {
            let elapsed = session
                .last_local_samples
                .saturating_sub(session.started_at_local_samples);
            (
                samples_to_beats(
                    session.started_at_local_samples,
                    session.bpm,
                    session.sample_rate,
                ),
                samples_to_beats(elapsed, session.bpm, session.sample_rate),
            )
        };
        Some(LoopRecordPreview {
            clip_id: session.preview_clip_id,
            target_id: session.target_id,
            track_id: session.track_id,
            position_beats,
            length_beats: length_beats.min(session.length_beats),
            notes,
        })
    }

    pub fn request_start(
        &mut self,
        target_id: T,
        track_id: TrackId,
        from_stopped: bool,
        length_beats: f64,
    ) -> Option<LoopRecordStartRequest<T>> {
        let bpm = self.bpm;
        let sample_rate = self.sample_rate;
        if self.is_active() || bpm <= 0.0 || sample_rate == 0 || length_beats <= 0.0 {
            return None;
        }
        self.session = Some(RecordingSession {
            preview_clip_id: ClipId::new(),
            target_id,
            track_id,
            bpm,
            sample_rate,
            length_beats,
            length_samples: (length_beats * f64::from(sample_rate) * 60.0 / bpm)
                .round()
                .max(1.0) as u64,
            mode: self.mode,
            quantization: self.quantization,
            started_at_samples: None,
            started_at_local_samples: 0,
            last_local_samples: 0,
            preview_wrapped: false,
            replace_wrapped: false,
            open_notes: Vec::new(),
            notes: Vec::new(),
        });
        self.phase = LoopRecordPhase::Preparing;
        self.arm_sent = false;
        Some(LoopRecordStartRequest {
            target_id,
            track_id,
            from_stopped,
            count_in_bars: if from_stopped {
                self.count_in.bars()
            } else {
                0
            },
            replace_existing: self.mode == LoopRecordMode::Replace,
        })
    }

    pub fn mark_arm_sent(&mut self) {
        if self.phase == LoopRecordPhase::Preparing {
            self.phase = LoopRecordPhase::Armed;
        }
        self.arm_sent = true;
    }

    pub fn arm_was_sent(&self) -> bool {
        self.arm_sent
    }

    pub fn arm(&mut self, target_id: T, track_id: TrackId, boundary: u64) {
        if self.target() == Some((target_id, track_id)) {
            self.phase = LoopRecordPhase::Armed;
            self.pending_boundary_samples = Some(boundary);
        }
    }

    pub fn start(
        &mut self,
        target_id: T,
        track_id: TrackId,
        effective_at_samples: u64,
        local_position_samples: u64,
    ) {
        if self.target() != Some((target_id, track_id)) {
            return;
        }
        let session = self.session.as_mut().expect("record target");
        session.started_at_samples = Some(effective_at_samples);
        session.started_at_local_samples = local_position_samples;
        session.last_local_samples = local_position_samples;
        self.phase = LoopRecordPhase::Recording;
        self.pending_boundary_samples = None;
    }

    pub fn observe_playhead(&mut self, target_id: T, position_samples: u64) {
        let Some(session) = self.session.as_mut().filter(|session| {
            session.target_id == target_id && session.started_at_samples.is_some()
        }) else {
            return;
        };
        if position_samples < session.last_local_samples {
            session.preview_wrapped = true;
            if session.mode == LoopRecordMode::Replace && !session.replace_wrapped {
                session.replace_wrapped = true;
                self.mode = LoopRecordMode::Overdub;
            }
        }
        session.last_local_samples = position_samples;
    }

    pub(crate) fn input_note(&mut self, input: LoopRecordInput<T>) {
        let LoopRecordInput {
            target_id,
            track_id,
            pitch,
            velocity,
            on,
            effective_at_samples,
            local_position_samples,
        } = input;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(started_at) = session.started_at_samples else {
            return;
        };
        if target_id != Some(session.target_id)
            || track_id != session.track_id
            || effective_at_samples < started_at
        {
            return;
        }
        let Some(local_position_samples) = local_position_samples else {
            return;
        };
        if on {
            session.last_local_samples = local_position_samples;
            session.open_notes.push(OpenNote {
                pitch,
                velocity,
                effective_at_samples,
                local_position_samples,
            });
        } else if let Some(index) = session
            .open_notes
            .iter()
            .rposition(|note| note.pitch == pitch)
        {
            let note = session.open_notes.remove(index);
            push_free_note(session, note, effective_at_samples);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn repeated_note(
        &mut self,
        target_id: Option<T>,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: NoteRepeatRate,
        effective_at_samples: u64,
        canonical_local_position_samples: Option<u64>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session
            .started_at_samples
            .is_none_or(|start| effective_at_samples < start)
            || target_id != Some(session.target_id)
            || track_id != session.track_id
        {
            return;
        }
        let Some(position) = canonical_local_position_samples else {
            return;
        };
        session.last_local_samples = position;
        session.notes.push(RecordedLoopNote {
            pitch,
            velocity,
            start_beat: samples_to_beats(position, session.bpm, session.sample_rate)
                .rem_euclid(session.length_beats),
            duration_beats: rate.interval_beats(),
            groove_grid: match rate {
                NoteRepeatRate::Eighth => GrooveGrid::Eighth,
                NoteRepeatRate::Sixteenth => GrooveGrid::Sixteenth,
                _ => GrooveGrid::Off,
            },
        });
    }

    pub fn request_stop(&mut self) -> bool {
        if !self.is_active() || self.phase == LoopRecordPhase::Stopping {
            return false;
        }
        self.phase = LoopRecordPhase::Stopping;
        true
    }

    pub fn finish(
        &mut self,
        target_id: T,
        track_id: TrackId,
        effective_at_samples: u64,
        local_position_samples: u64,
        started: bool,
    ) -> Option<CompletedLoopRecording<T>> {
        if self.target() != Some((target_id, track_id)) {
            return None;
        }
        let mut session = self.session.take().expect("record target");
        self.phase = LoopRecordPhase::Idle;
        self.pending_boundary_samples = None;
        self.arm_sent = false;
        if !started || session.started_at_samples.is_none() {
            return None;
        }
        for open in std::mem::take(&mut session.open_notes) {
            push_free_note(&mut session, open, effective_at_samples);
        }
        let replace_ranges = if session.mode != LoopRecordMode::Replace {
            Vec::new()
        } else {
            let start = samples_to_beats(
                session.started_at_local_samples,
                session.bpm,
                session.sample_rate,
            );
            let elapsed = effective_at_samples
                .saturating_sub(session.started_at_samples.expect("started loop recording"));
            let crossed_wrap = session.replace_wrapped
                || elapsed
                    >= session
                        .length_samples
                        .saturating_sub(session.started_at_local_samples);
            if crossed_wrap {
                vec![(start, session.length_beats)]
            } else {
                let end =
                    samples_to_beats(local_position_samples, session.bpm, session.sample_rate);
                (end > start).then_some((start, end)).into_iter().collect()
            }
        };
        Some(CompletedLoopRecording {
            target_id,
            track_id,
            notes: session.notes,
            replace_ranges,
        })
    }

    pub fn note_counts(&self) -> (usize, usize) {
        self.session.as_ref().map_or((0, 0), |session| {
            (session.notes.len(), session.open_notes.len())
        })
    }

    pub fn snapshot(&self, now: u64, local: u64) -> Option<CompletedLoopRecording<T>> {
        let (target, track) = self.target()?;
        let mut snapshot = self.clone();
        snapshot
            .session
            .as_mut()?
            .open_notes
            .retain(|note| note.effective_at_samples < now);
        snapshot.finish(target, track, now, local, true)
    }

    pub fn next_pass(&mut self, boundary: u64) -> Option<CompletedLoopRecording<T>> {
        let stopping = self.phase == LoopRecordPhase::Stopping;
        let session = self.session.as_ref()?;
        let (target, track, length) = (session.target_id, session.track_id, session.length_beats);
        let held = session.open_notes.clone();
        let completed = self.finish(target, track, boundary, 0, true);
        self.request_start(target, track, false, length)?;
        self.start(target, track, boundary, 0);
        for note in held {
            self.input_note(LoopRecordInput {
                target_id: Some(target),
                track_id: track,
                pitch: note.pitch,
                velocity: note.velocity,
                on: true,
                effective_at_samples: boundary,
                local_position_samples: Some(0),
            });
        }
        if stopping {
            self.phase = LoopRecordPhase::Stopping;
        }
        completed
    }

    pub fn cancel(&mut self) {
        self.session = None;
        self.phase = LoopRecordPhase::Idle;
        self.pending_boundary_samples = None;
        self.arm_sent = false;
    }
}

fn push_free_note<T>(session: &mut RecordingSession<T>, note: OpenNote, off_sample: u64) {
    let raw_start = samples_to_beats(
        note.local_position_samples,
        session.bpm,
        session.sample_rate,
    );
    let start_beat = quantize_start(raw_start, session.quantization, session.length_beats);
    let duration_beats = samples_to_beats(
        off_sample.saturating_sub(note.effective_at_samples),
        session.bpm,
        session.sample_rate,
    )
    .max(1.0 / 960.0);
    session.notes.push(RecordedLoopNote {
        pitch: note.pitch,
        velocity: note.velocity,
        start_beat,
        duration_beats,
        groove_grid: session.quantization.groove_grid(),
    });
}

fn preview_open_note<T>(session: &RecordingSession<T>, note: OpenNote) -> RecordedLoopNote {
    let raw_start = samples_to_beats(
        note.local_position_samples,
        session.bpm,
        session.sample_rate,
    );
    let elapsed_samples = if session.last_local_samples >= note.local_position_samples {
        session.last_local_samples - note.local_position_samples
    } else {
        session
            .length_samples
            .saturating_sub(note.local_position_samples)
            .saturating_add(session.last_local_samples)
    };
    RecordedLoopNote {
        pitch: note.pitch,
        velocity: note.velocity,
        start_beat: quantize_start(raw_start, session.quantization, session.length_beats),
        duration_beats: samples_to_beats(elapsed_samples, session.bpm, session.sample_rate)
            .max(1.0 / 960.0),
        groove_grid: session.quantization.groove_grid(),
    }
}

fn samples_to_beats(samples: u64, bpm: f64, sample_rate: u32) -> f64 {
    samples as f64 * bpm / (f64::from(sample_rate) * 60.0)
}

fn quantize_start(beat: f64, quantization: LoopRecordQuantization, length_beats: f64) -> f64 {
    let Some(step) = quantization.interval_beats() else {
        return beat.rem_euclid(length_beats);
    };
    let canonical = (beat / step).round() * step;
    canonical.rem_euclid(length_beats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(mode: LoopRecordMode, quantization: LoopRecordQuantization) -> LoopRecordState {
        let mut state = LoopRecordState {
            mode,
            quantization,
            ..LoopRecordState::default()
        };
        let target_id = SectionId::new();
        let track_id = TrackId::new();
        state.sync_clock(false, 60.0, 96);
        state
            .request_start(target_id, track_id, false, 4.0)
            .unwrap();
        state.start(target_id, track_id, 1_000, 0);
        state
    }

    fn input(
        state: &mut LoopRecordState,
        target: (Option<SectionId>, TrackId),
        note: (u8, u8, bool),
        timing: (u64, Option<u64>),
    ) {
        state.input_note(LoopRecordInput {
            target_id: target.0,
            track_id: target.1,
            pitch: note.0,
            velocity: note.1,
            on: note.2,
            effective_at_samples: timing.0,
            local_position_samples: timing.1,
        });
    }

    #[test]
    fn free_note_quantization_keeps_canonical_start_and_live_groove_grid() {
        let mut state = session(LoopRecordMode::Overdub, LoopRecordQuantization::Sixteenth);
        let (section, track) = state.target().unwrap();
        input(
            &mut state,
            (Some(section), track),
            (42, 100, true),
            (1_024, Some(24)),
        );
        input(
            &mut state,
            (Some(section), track),
            (42, 0, false),
            (1_072, Some(72)),
        );
        let completed = state.finish(section, track, 1_080, 80, true).unwrap();
        assert_eq!(completed.notes[0].start_beat, 0.25);
        assert_eq!(completed.notes[0].duration_beats, 0.5);
        assert_eq!(completed.notes[0].groove_grid, GrooveGrid::Sixteenth);
        assert_eq!(
            completed.notes[0]
                .groove_grid
                .map_beat(completed.notes[0].start_beat, SwingAmount::new(0.75)),
            0.375
        );
    }

    #[test]
    fn live_preview_shows_a_note_while_held_and_after_release() {
        let mut state = session(LoopRecordMode::Overdub, LoopRecordQuantization::Sixteenth);
        let (section, track) = state.target().unwrap();
        input(
            &mut state,
            (Some(section), track),
            (42, 100, true),
            (1_024, Some(24)),
        );

        let pressed = state.live_preview().expect("active recording preview");
        assert_eq!((pressed.target_id, pressed.track_id), (section, track));
        assert_eq!(pressed.notes.len(), 1);
        assert_eq!(pressed.notes[0].pitch, 42);
        assert_eq!(pressed.notes[0].start_beat, 0.25);
        assert_eq!(pressed.notes[0].duration_beats, 1.0 / 960.0);
        assert_eq!(pressed.position_beats, 0.0);
        assert_eq!(pressed.length_beats, 0.25);

        state.observe_playhead(section, 72);
        let held = state.live_preview().expect("held-note preview");
        assert_eq!(held.notes[0].duration_beats, 0.5);
        assert_eq!(held.length_beats, 0.75);

        input(
            &mut state,
            (Some(section), track),
            (42, 0, false),
            (1_072, Some(72)),
        );
        let released = state.live_preview().expect("released-note preview");
        assert_eq!(released.notes[0].duration_beats, 0.5);

        state.finish(section, track, 1_080, 80, true).unwrap();
        assert!(state.live_preview().is_none());
    }

    #[test]
    fn live_preview_grows_from_the_record_boundary_then_fills_after_wrap() {
        let mut state = LoopRecordState::default();
        let section = SectionId::new();
        let track = TrackId::new();
        state.sync_clock(true, 60.0, 96);
        state.request_start(section, track, false, 4.0).unwrap();
        state.start(section, track, 1_000, 96);

        state.observe_playhead(section, 144);
        let growing = state.live_preview().expect("first-pass preview");
        assert_eq!(growing.position_beats, 1.0);
        assert_eq!(growing.length_beats, 0.5);

        state.observe_playhead(section, 8);
        let wrapped = state.live_preview().expect("wrapped preview");
        assert_eq!(wrapped.position_beats, 0.0);
        assert_eq!(wrapped.length_beats, 4.0);
    }

    #[test]
    fn replace_erases_only_the_first_pass_before_becoming_overdub() {
        let mut state = session(LoopRecordMode::Replace, LoopRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        state.observe_playhead(section, 300);
        state.observe_playhead(section, 8);
        assert_eq!(state.mode, LoopRecordMode::Overdub);
        state.observe_playhead(section, 120);
        let completed = state.finish(section, track, 1_500, 120, true).unwrap();
        assert_eq!(completed.replace_ranges, vec![(0.0, 4.0)]);
    }

    #[test]
    fn replace_start_request_carries_the_live_suppression_policy() {
        let mut state = LoopRecordState {
            mode: LoopRecordMode::Replace,
            ..LoopRecordState::default()
        };
        state.sync_clock(true, 120.0, 48_000);
        let request = state
            .request_start(SectionId::new(), TrackId::new(), false, 4.0)
            .unwrap();

        assert!(request.replace_existing);
    }

    #[test]
    fn replace_detects_a_whole_first_pass_from_engine_elapsed_time() {
        let mut state = session(LoopRecordMode::Replace, LoopRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        let completed = state.finish(section, track, 1_384, 0, true).unwrap();
        assert_eq!(completed.replace_ranges, vec![(0.0, 4.0)]);
    }

    #[test]
    fn repeat_keeps_canonical_position_and_matching_groove_grid() {
        let mut state = session(LoopRecordMode::Overdub, LoopRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        state.repeated_note(
            Some(section),
            track,
            42,
            100,
            NoteRepeatRate::Sixteenth,
            1_032,
            Some(24),
        );
        let completed = state.finish(section, track, 1_040, 40, true).unwrap();
        assert_eq!(completed.notes[0].start_beat, 0.25);
        assert_eq!(completed.notes[0].groove_grid, GrooveGrid::Sixteenth);
    }

    #[test]
    fn every_input_grid_snaps_while_triplets_remain_exact() {
        let cases = [
            (LoopRecordQuantization::Off, 0.48, 0.48),
            (LoopRecordQuantization::Quarter, 0.48, 0.0),
            (LoopRecordQuantization::QuarterTriplet, 0.7, 2.0 / 3.0),
            (LoopRecordQuantization::Eighth, 0.48, 0.5),
            (LoopRecordQuantization::EighthTriplet, 0.31, 1.0 / 3.0),
            (LoopRecordQuantization::Sixteenth, 0.24, 0.25),
            (LoopRecordQuantization::SixteenthTriplet, 0.18, 1.0 / 6.0),
            (LoopRecordQuantization::ThirtySecond, 0.12, 0.125),
            (
                LoopRecordQuantization::ThirtySecondTriplet,
                0.09,
                1.0 / 12.0,
            ),
        ];
        for (grid, input, expected) in cases {
            let actual = quantize_start(input, grid, 4.0);
            assert!((actual - expected).abs() < 1e-9, "{grid:?}: {actual}");
        }
    }

    #[test]
    fn mismatched_section_or_track_never_redirects_the_fixed_target() {
        let mut state = session(LoopRecordMode::Overdub, LoopRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        input(
            &mut state,
            (Some(SectionId::new()), track),
            (42, 100, true),
            (1_010, Some(10)),
        );
        input(
            &mut state,
            (Some(section), TrackId::new()),
            (42, 100, true),
            (1_020, Some(20)),
        );
        input(
            &mut state,
            (Some(section), track),
            (42, 100, true),
            (1_030, Some(30)),
        );
        input(
            &mut state,
            (Some(section), track),
            (42, 0, false),
            (1_040, Some(40)),
        );

        let completed = state.finish(section, track, 1_050, 50, true).unwrap();
        assert_eq!(completed.notes.len(), 1);
        assert_eq!(completed.track_id, track);
        assert_eq!(completed.target_id, section);
    }
}
