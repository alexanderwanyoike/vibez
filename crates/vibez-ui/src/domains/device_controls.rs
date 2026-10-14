//! Native device disclosure and numeric-entry transitions independent of rendering.

use std::collections::{BTreeMap, BTreeSet};
use vibez_core::effect::ParamDescriptor;
use vibez_core::id::{EffectId, TrackId};

pub fn presentation_descriptor(effect: &crate::state::UiEffect, index: usize) -> ParamDescriptor {
    let mut descriptor = effect.descriptors[index];
    if crate::domains::gate::is_native_gate(effect) && index == vibez_dsp::gate::param::MIX {
        descriptor.unit = "%";
    }
    descriptor
}

#[derive(Debug, Clone)]
pub enum DeviceControlMsg {
    ToggleAdvanced(TrackId, EffectId),
    Input(TrackId, EffectId, usize, String),
    Submit(TrackId, EffectId, usize),
}

impl DeviceControlMsg {
    pub fn destination(&self) -> (TrackId, EffectId) {
        match self {
            Self::ToggleAdvanced(track, effect)
            | Self::Input(track, effect, ..)
            | Self::Submit(track, effect, ..) => (*track, *effect),
        }
    }
}

#[derive(Default)]
pub struct DeviceControlState {
    pub advanced: bool,
    pub entries: BTreeMap<usize, String>,
    pub errors: BTreeSet<usize>,
}

impl DeviceControlState {
    pub fn discard(&mut self, index: usize) {
        self.entries.remove(&index);
        self.errors.remove(&index);
    }

    pub fn update(
        &mut self,
        message: DeviceControlMsg,
        descriptors: &[ParamDescriptor],
        values: &[f32],
    ) -> Option<(usize, f32)> {
        match message {
            DeviceControlMsg::ToggleAdvanced(..) => self.advanced = !self.advanced,
            DeviceControlMsg::Input(_, _, index, text) if index < descriptors.len() => {
                self.entries.insert(index, text);
                self.errors.remove(&index);
            }
            DeviceControlMsg::Submit(_, _, index) => {
                let descriptor = descriptors.get(index)?;
                let Some(value) = self
                    .entries
                    .get(&index)
                    .and_then(|text| parse_value(text, descriptor))
                else {
                    self.errors.insert(index);
                    return None;
                };
                self.discard(index);
                if values.get(index).copied().unwrap_or(descriptor.default) != value {
                    return Some((index, value));
                }
            }
            _ => {}
        }
        None
    }
}

pub fn parse_value(text: &str, descriptor: &ParamDescriptor) -> Option<f32> {
    let text = text.trim().to_ascii_lowercase();
    let text = text
        .trim_end_matches(descriptor.unit.to_ascii_lowercase().as_str())
        .trim();
    let (number, multiplier) = if descriptor.unit == "Hz" && text.ends_with('k') {
        (text.trim_end_matches('k'), 1000.0)
    } else {
        (text, 1.0)
    };
    let value = number.trim().parse::<f32>().ok()? * multiplier;
    let value = if descriptor.unit == "%" {
        value / 100.0
    } else {
        value
    };
    (value.is_finite() && (descriptor.min..=descriptor.max).contains(&value)).then_some(value)
}

pub fn display_value(value: f32, descriptor: &ParamDescriptor) -> String {
    if descriptor.unit == "%" {
        format!("{:.0}", value * 100.0)
    } else if descriptor.unit == "Hz" {
        if value >= 1000.0 {
            format!("{:.2}k", value / 1000.0)
        } else if descriptor.max <= 20.0 {
            format!("{value:.2}")
        } else {
            format!("{value:.0}")
        }
    } else {
        format!("{value:.2}")
    }
}

pub fn log_position(value: f32, min: f32, max: f32) -> f32 {
    (value.clamp(min, max) / min).ln() / (max / min).ln()
}

pub fn log_value(position: f32, min: f32, max: f32) -> f32 {
    min * (max / min).powf(position.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    const FREQUENCY: ParamDescriptor = ParamDescriptor {
        name: "Cutoff",
        min: 20.0,
        max: 20000.0,
        default: 1000.0,
        unit: "Hz",
    };
    #[test]
    fn invalid_and_unchanged_entries_do_not_produce_edits() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let mut state = DeviceControlState::default();
        for input in ["NaN", "-20", "oops", "20001"] {
            state.update(
                DeviceControlMsg::Input(track, effect, 0, input.into()),
                &[FREQUENCY],
                &[1000.0],
            );
            assert_eq!(
                state.update(
                    DeviceControlMsg::Submit(track, effect, 0),
                    &[FREQUENCY],
                    &[1000.0]
                ),
                None
            );
            assert!(state.errors.contains(&0));
        }
        state.update(
            DeviceControlMsg::Input(track, effect, 0, "1kHz".into()),
            &[FREQUENCY],
            &[1000.0],
        );
        assert_eq!(
            state.update(
                DeviceControlMsg::Submit(track, effect, 0),
                &[FREQUENCY],
                &[1000.0]
            ),
            None
        );
        assert!(state.errors.is_empty());
        state.update(
            DeviceControlMsg::Input(track, effect, 0, "1.2k".into()),
            &[FREQUENCY],
            &[1000.0],
        );
        assert_eq!(
            state.update(
                DeviceControlMsg::Submit(track, effect, 0),
                &[FREQUENCY],
                &[1000.0]
            ),
            Some((0, 1200.0))
        );
    }
    #[test]
    fn logarithmic_frequency_mapping_is_invertible() {
        for frequency in [20.0, 200.0, 1000.0, 18000.0, 20000.0] {
            let restored = log_value(log_position(frequency, 20.0, 20000.0), 20.0, 20000.0);
            assert!((restored / frequency - 1.0).abs() < 1e-5);
        }
    }
}
