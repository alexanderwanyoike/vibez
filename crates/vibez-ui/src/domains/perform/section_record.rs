use vibez_core::id::{SectionId, TrackId};
#[cfg(test)]
use vibez_core::perform::SwingAmount;
use vibez_core::perform::{GrooveGrid, NoteRepeatRate};

use super::{PerformAction, PerformState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionRecordCountIn {
    Off,
    #[default]
    OneBar,
    TwoBars,
}

impl SectionRecordCountIn {
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

impl std::fmt::Display for SectionRecordCountIn {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionRecordMode {
    #[default]
    Overdub,
    Replace,
}

impl SectionRecordMode {
    pub const ALL: [Self; 2] = [Self::Overdub, Self::Replace];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Overdub => "Overdub",
            Self::Replace => "Replace",
        }
    }
}

impl std::fmt::Display for SectionRecordMode {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionRecordQuantization {
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

impl SectionRecordQuantization {
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

impl std::fmt::Display for SectionRecordQuantization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SectionRecordPhase {
    #[default]
    Idle,
    Preparing,
    Armed,
    Recording,
    Stopping,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionRecordMsg {
    Toggle,
    SetCountIn(SectionRecordCountIn),
    SetMode(SectionRecordMode),
    SetQuantization(SectionRecordQuantization),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionRecordAction {
    Start(SectionRecordStartRequest),
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SectionRecordStartRequest {
    pub section_id: SectionId,
    pub track_id: TrackId,
    pub from_stopped: bool,
    pub count_in_bars: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RecordedSectionNote {
    pub pitch: u8,
    pub velocity: u8,
    pub start_beat: f64,
    pub duration_beats: f64,
    pub groove_grid: GrooveGrid,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SectionRecordInput {
    pub section_id: Option<SectionId>,
    pub track_id: TrackId,
    pub pitch: u8,
    pub velocity: u8,
    pub on: bool,
    pub effective_at_samples: u64,
    pub section_position_samples: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompletedSectionRecording {
    pub section_id: SectionId,
    pub track_id: TrackId,
    pub notes: Vec<RecordedSectionNote>,
    pub replace_ranges: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy)]
struct OpenNote {
    pitch: u8,
    velocity: u8,
    effective_at_samples: u64,
    section_position_samples: u64,
}

#[derive(Debug)]
struct RecordingSession {
    section_id: SectionId,
    track_id: TrackId,
    bpm: f64,
    sample_rate: u32,
    length_beats: f64,
    length_samples: u64,
    mode: SectionRecordMode,
    quantization: SectionRecordQuantization,
    started_at_samples: Option<u64>,
    started_at_section_samples: u64,
    last_section_samples: u64,
    replace_wrapped: bool,
    open_notes: Vec<OpenNote>,
    notes: Vec<RecordedSectionNote>,
}

#[derive(Debug)]
pub struct SectionRecordState {
    pub count_in: SectionRecordCountIn,
    pub mode: SectionRecordMode,
    pub quantization: SectionRecordQuantization,
    pub phase: SectionRecordPhase,
    pub pending_boundary_samples: Option<u64>,
    arm_sent: bool,
    transport_playing: bool,
    bpm: f64,
    sample_rate: u32,
    session: Option<RecordingSession>,
}

impl Default for SectionRecordState {
    fn default() -> Self {
        Self {
            count_in: SectionRecordCountIn::default(),
            mode: SectionRecordMode::default(),
            quantization: SectionRecordQuantization::default(),
            phase: SectionRecordPhase::Idle,
            pending_boundary_samples: None,
            arm_sent: false,
            transport_playing: false,
            bpm: 120.0,
            sample_rate: 44_100,
            session: None,
        }
    }
}

impl SectionRecordState {
    pub fn sync_clock(&mut self, transport_playing: bool, bpm: f64, sample_rate: u32) {
        self.transport_playing = transport_playing;
        self.bpm = bpm;
        self.sample_rate = sample_rate;
    }

    pub fn is_active(&self) -> bool {
        self.phase != SectionRecordPhase::Idle
    }

    pub fn target(&self) -> Option<(SectionId, TrackId)> {
        self.session
            .as_ref()
            .map(|session| (session.section_id, session.track_id))
    }

    pub fn request_start(
        &mut self,
        section_id: SectionId,
        track_id: TrackId,
        from_stopped: bool,
        length_beats: f64,
    ) -> Option<SectionRecordStartRequest> {
        let bpm = self.bpm;
        let sample_rate = self.sample_rate;
        if self.is_active() || bpm <= 0.0 || sample_rate == 0 || length_beats <= 0.0 {
            return None;
        }
        self.session = Some(RecordingSession {
            section_id,
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
            started_at_section_samples: 0,
            last_section_samples: 0,
            replace_wrapped: false,
            open_notes: Vec::new(),
            notes: Vec::new(),
        });
        self.phase = SectionRecordPhase::Preparing;
        self.arm_sent = false;
        Some(SectionRecordStartRequest {
            section_id,
            track_id,
            from_stopped,
            count_in_bars: if from_stopped {
                self.count_in.bars()
            } else {
                0
            },
        })
    }

    pub fn mark_arm_sent(&mut self) {
        if self.phase == SectionRecordPhase::Preparing {
            self.phase = SectionRecordPhase::Armed;
        }
        self.arm_sent = true;
    }

    pub fn arm_was_sent(&self) -> bool {
        self.arm_sent
    }

    pub fn arm(&mut self, section_id: SectionId, track_id: TrackId, boundary: u64) {
        if self.target() == Some((section_id, track_id)) {
            self.phase = SectionRecordPhase::Armed;
            self.pending_boundary_samples = Some(boundary);
        }
    }

    pub fn start(
        &mut self,
        section_id: SectionId,
        track_id: TrackId,
        effective_at_samples: u64,
        section_position_samples: u64,
    ) {
        if self.target() != Some((section_id, track_id)) {
            return;
        }
        let session = self.session.as_mut().expect("record target");
        session.started_at_samples = Some(effective_at_samples);
        session.started_at_section_samples = section_position_samples;
        session.last_section_samples = section_position_samples;
        self.phase = SectionRecordPhase::Recording;
        self.pending_boundary_samples = None;
    }

    pub fn observe_playhead(&mut self, section_id: SectionId, position_samples: u64) {
        let Some(session) = self.session.as_mut().filter(|session| {
            session.section_id == section_id && session.started_at_samples.is_some()
        }) else {
            return;
        };
        if session.mode == SectionRecordMode::Replace
            && !session.replace_wrapped
            && position_samples < session.last_section_samples
        {
            session.replace_wrapped = true;
            self.mode = SectionRecordMode::Overdub;
        }
        session.last_section_samples = position_samples;
    }

    pub(crate) fn input_note(&mut self, input: SectionRecordInput) {
        let SectionRecordInput {
            section_id,
            track_id,
            pitch,
            velocity,
            on,
            effective_at_samples,
            section_position_samples,
        } = input;
        let Some(session) = self.session.as_mut() else {
            return;
        };
        let Some(started_at) = session.started_at_samples else {
            return;
        };
        if section_id != Some(session.section_id)
            || track_id != session.track_id
            || effective_at_samples < started_at
        {
            return;
        }
        let Some(section_position_samples) = section_position_samples else {
            return;
        };
        if on {
            session.open_notes.push(OpenNote {
                pitch,
                velocity,
                effective_at_samples,
                section_position_samples,
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
        section_id: Option<SectionId>,
        track_id: TrackId,
        pitch: u8,
        velocity: u8,
        rate: NoteRepeatRate,
        effective_at_samples: u64,
        canonical_section_position_samples: Option<u64>,
    ) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if session
            .started_at_samples
            .is_none_or(|start| effective_at_samples < start)
            || section_id != Some(session.section_id)
            || track_id != session.track_id
        {
            return;
        }
        let Some(position) = canonical_section_position_samples else {
            return;
        };
        session.notes.push(RecordedSectionNote {
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
        if !self.is_active() || self.phase == SectionRecordPhase::Stopping {
            return false;
        }
        self.phase = SectionRecordPhase::Stopping;
        true
    }

    pub fn finish(
        &mut self,
        section_id: SectionId,
        track_id: TrackId,
        effective_at_samples: u64,
        section_position_samples: u64,
        started: bool,
    ) -> Option<CompletedSectionRecording> {
        if self.target() != Some((section_id, track_id)) {
            return None;
        }
        let mut session = self.session.take().expect("record target");
        self.phase = SectionRecordPhase::Idle;
        self.pending_boundary_samples = None;
        self.arm_sent = false;
        if !started || session.started_at_samples.is_none() {
            return None;
        }
        for open in std::mem::take(&mut session.open_notes) {
            push_free_note(&mut session, open, effective_at_samples);
        }
        let replace_ranges = if session.mode != SectionRecordMode::Replace {
            Vec::new()
        } else {
            let start = samples_to_beats(
                session.started_at_section_samples,
                session.bpm,
                session.sample_rate,
            );
            let elapsed = effective_at_samples.saturating_sub(
                session
                    .started_at_samples
                    .expect("started Section recording"),
            );
            let crossed_wrap = session.replace_wrapped
                || elapsed
                    >= session
                        .length_samples
                        .saturating_sub(session.started_at_section_samples);
            if crossed_wrap {
                vec![(start, session.length_beats)]
            } else {
                let end =
                    samples_to_beats(section_position_samples, session.bpm, session.sample_rate);
                (end > start).then_some((start, end)).into_iter().collect()
            }
        };
        Some(CompletedSectionRecording {
            section_id,
            track_id,
            notes: session.notes,
            replace_ranges,
        })
    }

    pub fn cancel(&mut self) {
        self.session = None;
        self.phase = SectionRecordPhase::Idle;
        self.pending_boundary_samples = None;
        self.arm_sent = false;
    }
}

impl PerformState {
    pub(super) fn update_section_record(&mut self, msg: SectionRecordMsg) -> PerformAction {
        match msg {
            SectionRecordMsg::SetCountIn(value) if !self.section_record.is_active() => {
                self.section_record.count_in = value;
            }
            SectionRecordMsg::SetMode(value) if !self.section_record.is_active() => {
                self.section_record.mode = value;
            }
            SectionRecordMsg::SetQuantization(value) if !self.section_record.is_active() => {
                self.section_record.quantization = value;
            }
            SectionRecordMsg::Toggle if self.section_record.is_active() => {
                if self.section_record.request_stop() {
                    return PerformAction {
                        section_record: Some(SectionRecordAction::Stop),
                        ..PerformAction::default()
                    };
                }
            }
            SectionRecordMsg::Toggle => {
                let Some(track_id) = self.instrument_target() else {
                    return PerformAction {
                        section_record_status: Some("Choose an Instrument Target before recording"),
                        ..PerformAction::default()
                    };
                };
                let section_id = if self.section_record.transport_playing {
                    self.playing_section
                } else {
                    self.selected_section
                };
                let Some(section_id) = section_id else {
                    return PerformAction {
                        section_record_status: Some(if self.section_record.transport_playing {
                            "Section Record requires a playing Section"
                        } else {
                            "Select a Section before recording"
                        }),
                        ..PerformAction::default()
                    };
                };
                let Some(section) = self.sections.by_id(section_id) else {
                    return PerformAction::default();
                };
                if let Some(request) = self.section_record.request_start(
                    section_id,
                    track_id,
                    !self.section_record.transport_playing,
                    section.length_beats,
                ) {
                    return PerformAction {
                        section_record: Some(SectionRecordAction::Start(request)),
                        ..PerformAction::default()
                    };
                }
            }
            _ => {}
        }
        PerformAction::default()
    }
}

fn push_free_note(session: &mut RecordingSession, note: OpenNote, off_sample: u64) {
    let raw_start = samples_to_beats(
        note.section_position_samples,
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
    session.notes.push(RecordedSectionNote {
        pitch: note.pitch,
        velocity: note.velocity,
        start_beat,
        duration_beats,
        groove_grid: session.quantization.groove_grid(),
    });
}

fn samples_to_beats(samples: u64, bpm: f64, sample_rate: u32) -> f64 {
    samples as f64 * bpm / (f64::from(sample_rate) * 60.0)
}

fn quantize_start(beat: f64, quantization: SectionRecordQuantization, length_beats: f64) -> f64 {
    let Some(step) = quantization.interval_beats() else {
        return beat.rem_euclid(length_beats);
    };
    let canonical = (beat / step).round() * step;
    canonical.rem_euclid(length_beats)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(
        mode: SectionRecordMode,
        quantization: SectionRecordQuantization,
    ) -> SectionRecordState {
        let mut state = SectionRecordState {
            mode,
            quantization,
            ..SectionRecordState::default()
        };
        let section_id = SectionId::new();
        let track_id = TrackId::new();
        state.sync_clock(false, 60.0, 96);
        state
            .request_start(section_id, track_id, false, 4.0)
            .unwrap();
        state.start(section_id, track_id, 1_000, 0);
        state
    }

    fn input(
        state: &mut SectionRecordState,
        target: (Option<SectionId>, TrackId),
        note: (u8, u8, bool),
        timing: (u64, Option<u64>),
    ) {
        state.input_note(SectionRecordInput {
            section_id: target.0,
            track_id: target.1,
            pitch: note.0,
            velocity: note.1,
            on: note.2,
            effective_at_samples: timing.0,
            section_position_samples: timing.1,
        });
    }

    #[test]
    fn free_note_quantization_keeps_canonical_start_and_live_groove_grid() {
        let mut state = session(
            SectionRecordMode::Overdub,
            SectionRecordQuantization::Sixteenth,
        );
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
    fn replace_erases_only_the_first_pass_before_becoming_overdub() {
        let mut state = session(SectionRecordMode::Replace, SectionRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        state.observe_playhead(section, 300);
        state.observe_playhead(section, 8);
        assert_eq!(state.mode, SectionRecordMode::Overdub);
        state.observe_playhead(section, 120);
        let completed = state.finish(section, track, 1_500, 120, true).unwrap();
        assert_eq!(completed.replace_ranges, vec![(0.0, 4.0)]);
    }

    #[test]
    fn replace_detects_a_whole_first_pass_from_engine_elapsed_time() {
        let mut state = session(SectionRecordMode::Replace, SectionRecordQuantization::Off);
        let (section, track) = state.target().unwrap();
        let completed = state.finish(section, track, 1_384, 0, true).unwrap();
        assert_eq!(completed.replace_ranges, vec![(0.0, 4.0)]);
    }

    #[test]
    fn repeat_keeps_canonical_position_and_matching_groove_grid() {
        let mut state = session(SectionRecordMode::Overdub, SectionRecordQuantization::Off);
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
            (SectionRecordQuantization::Off, 0.48, 0.48),
            (SectionRecordQuantization::Quarter, 0.48, 0.0),
            (SectionRecordQuantization::QuarterTriplet, 0.7, 2.0 / 3.0),
            (SectionRecordQuantization::Eighth, 0.48, 0.5),
            (SectionRecordQuantization::EighthTriplet, 0.31, 1.0 / 3.0),
            (SectionRecordQuantization::Sixteenth, 0.24, 0.25),
            (SectionRecordQuantization::SixteenthTriplet, 0.18, 1.0 / 6.0),
            (SectionRecordQuantization::ThirtySecond, 0.12, 0.125),
            (
                SectionRecordQuantization::ThirtySecondTriplet,
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
        let mut state = session(SectionRecordMode::Overdub, SectionRecordQuantization::Off);
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
        assert_eq!(completed.section_id, section);
    }
}
