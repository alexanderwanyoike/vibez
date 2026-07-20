//! Instrument-mode note transformation and live performance controls.

use std::fmt;

use vibez_core::id::TrackId;

use crate::state::ProjectTrack;

use super::{PadPosition, PerformMode, PerformState};

const INSTRUMENT_BASE_PITCH: u8 = 35;
const MIN_INSTRUMENT_OCTAVE: i8 = -3;
const MAX_INSTRUMENT_OCTAVE: i8 = 6;
const LEVEL_COUNT: i16 = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ActiveInstrumentNote {
    pub(super) track_id: TrackId,
    pub(super) pitch: u8,
    pub(super) velocity: u8,
    pub(super) repeat_id: u8,
    pub(super) repeating: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ComputerKeyVelocity(pub(super) u8);

impl Default for ComputerKeyVelocity {
    fn default() -> Self {
        Self(100)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SixteenLevelsParameter {
    #[default]
    Pitch,
    Velocity,
}

impl SixteenLevelsParameter {
    pub const ALL: [Self; 2] = [Self::Pitch, Self::Velocity];

    fn descriptor(self) -> &'static SixteenLevelsParameterDescriptor {
        SIXTEEN_LEVELS_PARAMETERS
            .iter()
            .find(|descriptor| descriptor.parameter == self)
            .expect("every 16 Levels parameter has a descriptor")
    }
}

impl fmt::Display for SixteenLevelsParameter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.descriptor().label)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SixteenLevelsRange {
    pub minimum: i16,
    pub maximum: i16,
}

impl SixteenLevelsRange {
    fn value_at(self, position: PadPosition) -> i16 {
        let index = i16::from(position.ordinal(PerformMode::Instrument) - 1);
        let span = self.maximum - self.minimum;
        self.minimum + (span * index + (LEVEL_COUNT - 1) / 2) / (LEVEL_COUNT - 1)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InstrumentPadPreview {
    pub pitch: u8,
    pub velocity: u8,
}

type ApplyParameter = fn(InstrumentPadPreview, i16) -> InstrumentPadPreview;

struct SixteenLevelsParameterDescriptor {
    parameter: SixteenLevelsParameter,
    label: &'static str,
    bounds: SixteenLevelsRange,
    default_range: SixteenLevelsRange,
    owns_velocity: bool,
    apply: ApplyParameter,
}

const SIXTEEN_LEVELS_PARAMETERS: [SixteenLevelsParameterDescriptor; 2] = [
    SixteenLevelsParameterDescriptor {
        parameter: SixteenLevelsParameter::Pitch,
        label: "Pitch",
        bounds: SixteenLevelsRange {
            minimum: -48,
            maximum: 48,
        },
        default_range: SixteenLevelsRange {
            minimum: 0,
            maximum: 15,
        },
        owns_velocity: false,
        apply: apply_pitch,
    },
    SixteenLevelsParameterDescriptor {
        parameter: SixteenLevelsParameter::Velocity,
        label: "Velocity",
        bounds: SixteenLevelsRange {
            minimum: 1,
            maximum: 127,
        },
        default_range: SixteenLevelsRange {
            minimum: 8,
            maximum: 127,
        },
        owns_velocity: true,
        apply: apply_velocity,
    },
];

fn apply_pitch(note: InstrumentPadPreview, value: i16) -> InstrumentPadPreview {
    InstrumentPadPreview {
        pitch: (i16::from(note.pitch) + value).clamp(0, 127) as u8,
        ..note
    }
}

fn apply_velocity(note: InstrumentPadPreview, value: i16) -> InstrumentPadPreview {
    InstrumentPadPreview {
        velocity: value.clamp(1, 127) as u8,
        ..note
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SixteenLevelsAssignment {
    parameter: SixteenLevelsParameter,
    range: SixteenLevelsRange,
}

impl Default for SixteenLevelsAssignment {
    fn default() -> Self {
        let parameter = SixteenLevelsParameter::default();
        Self {
            parameter,
            range: parameter.descriptor().default_range,
        }
    }
}

impl SixteenLevelsAssignment {
    fn map(
        self,
        source_pitch: u8,
        input_velocity: u8,
        position: PadPosition,
    ) -> InstrumentPadPreview {
        let descriptor = self.parameter.descriptor();
        let note = InstrumentPadPreview {
            pitch: source_pitch,
            velocity: input_velocity,
        };
        (descriptor.apply)(note, self.range.value_at(position))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct InstrumentPerformanceState {
    octave: i8,
    computer_key_velocity: ComputerKeyVelocity,
    full_level: bool,
    sixteen_levels_enabled: bool,
    sixteen_levels: SixteenLevelsAssignment,
    source_pitch: Option<u8>,
    last_played_pitch: Option<u8>,
    choosing_source: bool,
    target: Option<TrackId>,
}

impl PerformState {
    pub(crate) const fn instrument_target(&self) -> Option<TrackId> {
        self.instrument.target
    }

    pub(crate) fn sync_instrument_target_from_selection(
        &mut self,
        selected: Option<TrackId>,
        project_tracks: &[ProjectTrack],
    ) {
        let playable_target = selected.filter(|track_id| {
            project_tracks
                .iter()
                .any(|track| track.id == *track_id && track.is_playable_midi_target())
        });
        if playable_target.is_some() {
            self.sync_instrument_target(playable_target);
        }
    }

    pub(crate) fn sync_instrument_target(&mut self, target: Option<TrackId>) {
        if self.instrument.target != target {
            self.instrument.target = target;
            self.clear_instrument_source();
        }
    }

    pub(super) fn clear_instrument_source(&mut self) {
        self.instrument.source_pitch = None;
        self.instrument.last_played_pitch = None;
        self.instrument.choosing_source = self.instrument.sixteen_levels_enabled;
    }

    pub(super) fn shift_instrument_octave(&mut self, amount: i8) {
        self.instrument.octave = self
            .instrument
            .octave
            .saturating_add(amount)
            .clamp(MIN_INSTRUMENT_OCTAVE, MAX_INSTRUMENT_OCTAVE);
    }

    pub(super) fn toggle_full_level(&mut self) {
        if self.full_level_available() {
            self.instrument.full_level = !self.instrument.full_level;
        }
    }

    pub(super) fn toggle_sixteen_levels(&mut self) {
        self.instrument.sixteen_levels_enabled = !self.instrument.sixteen_levels_enabled;
        if self.instrument.sixteen_levels_enabled {
            self.instrument.source_pitch = self.instrument.last_played_pitch;
            self.instrument.choosing_source = self.instrument.source_pitch.is_none();
        } else {
            self.instrument.choosing_source = false;
        }
    }

    pub(super) fn select_sixteen_levels_parameter(&mut self, parameter: SixteenLevelsParameter) {
        if self.instrument.sixteen_levels.parameter != parameter {
            self.instrument.sixteen_levels = SixteenLevelsAssignment {
                parameter,
                range: parameter.descriptor().default_range,
            };
        }
    }

    pub(super) fn set_sixteen_levels_minimum(&mut self, minimum: i16) {
        let descriptor = self.instrument.sixteen_levels.parameter.descriptor();
        self.instrument.sixteen_levels.range.minimum = minimum
            .clamp(descriptor.bounds.minimum, descriptor.bounds.maximum)
            .min(self.instrument.sixteen_levels.range.maximum);
    }

    pub(super) fn set_sixteen_levels_maximum(&mut self, maximum: i16) {
        let descriptor = self.instrument.sixteen_levels.parameter.descriptor();
        self.instrument.sixteen_levels.range.maximum = maximum
            .clamp(descriptor.bounds.minimum, descriptor.bounds.maximum)
            .max(self.instrument.sixteen_levels.range.minimum);
    }

    pub(super) fn begin_choosing_sixteen_levels_source(&mut self) {
        if self.instrument.sixteen_levels_enabled {
            self.instrument.choosing_source = true;
        }
    }

    pub(super) fn resolve_instrument_note(
        &mut self,
        position: PadPosition,
        input_velocity: u8,
        track_id: TrackId,
    ) -> ActiveInstrumentNote {
        self.sync_instrument_target(Some(track_id));
        let pitch = self.instrument_pitch(position);
        let input_velocity = input_velocity.clamp(1, 127);
        let velocity = if self.full_level_effective() {
            127
        } else {
            input_velocity
        };

        let preview = if self.instrument.sixteen_levels_enabled {
            if let (false, Some(source_pitch)) = (
                self.instrument.choosing_source,
                self.instrument.source_pitch,
            ) {
                self.instrument
                    .sixteen_levels
                    .map(source_pitch, velocity, position)
            } else {
                self.instrument.source_pitch = Some(pitch);
                self.instrument.last_played_pitch = Some(pitch);
                self.instrument.choosing_source = false;
                InstrumentPadPreview { pitch, velocity }
            }
        } else {
            self.instrument.last_played_pitch = Some(pitch);
            InstrumentPadPreview { pitch, velocity }
        };

        ActiveInstrumentNote {
            track_id,
            pitch: preview.pitch,
            velocity: preview.velocity,
            repeat_id: position.index() as u8,
            repeating: false,
        }
    }

    pub fn instrument_pad_preview(&self, position: PadPosition) -> InstrumentPadPreview {
        let pitch = self.instrument_pitch(position);
        let velocity = if self.full_level_effective() {
            127
        } else {
            self.fixed_computer_velocity()
        };
        if self.instrument.sixteen_levels_enabled && !self.instrument.choosing_source {
            if let Some(source_pitch) = self.instrument.source_pitch {
                return self
                    .instrument
                    .sixteen_levels
                    .map(source_pitch, velocity, position);
            }
        }
        InstrumentPadPreview { pitch, velocity }
    }

    pub const fn fixed_computer_velocity(&self) -> u8 {
        self.instrument.computer_key_velocity.0
    }

    pub const fn instrument_octave(&self) -> i8 {
        self.instrument.octave
    }

    pub fn instrument_pitch(&self, position: PadPosition) -> u8 {
        let base = i16::from(INSTRUMENT_BASE_PITCH + position.ordinal(PerformMode::Instrument));
        (base + i16::from(self.instrument.octave) * 12).clamp(0, 127) as u8
    }

    pub fn set_fixed_computer_velocity(&mut self, velocity: u8) {
        self.instrument.computer_key_velocity = ComputerKeyVelocity(velocity.clamp(1, 127));
    }

    pub const fn full_level_enabled(&self) -> bool {
        self.instrument.full_level
    }

    pub fn full_level_available(&self) -> bool {
        !(self.instrument.sixteen_levels_enabled
            && self
                .instrument
                .sixteen_levels
                .parameter
                .descriptor()
                .owns_velocity)
    }

    pub fn full_level_effective(&self) -> bool {
        self.instrument.full_level && self.full_level_available()
    }

    pub const fn sixteen_levels_enabled(&self) -> bool {
        self.instrument.sixteen_levels_enabled
    }

    pub const fn sixteen_levels_parameter(&self) -> SixteenLevelsParameter {
        self.instrument.sixteen_levels.parameter
    }

    pub const fn sixteen_levels_range(&self) -> SixteenLevelsRange {
        self.instrument.sixteen_levels.range
    }

    pub fn sixteen_levels_bounds(&self) -> SixteenLevelsRange {
        self.instrument.sixteen_levels.parameter.descriptor().bounds
    }

    pub const fn sixteen_levels_source_pitch(&self) -> Option<u8> {
        self.instrument.source_pitch
    }

    pub const fn choosing_sixteen_levels_source(&self) -> bool {
        self.instrument.choosing_source
    }
}

#[cfg(test)]
#[path = "instrument_tests.rs"]
mod tests;
