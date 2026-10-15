//! Typed entry for built-in device parameters: how each value reads and
//! what typed text means, with one entry open across all devices.

use vibez_core::effect::{EffectType, ParamDescriptor};
use vibez_core::id::{EffectId, TrackId};

/// How a stored parameter value is shown and typed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParamFormat {
    Decibels,
    /// A linear amplitude shown in decibels, where 0 is "-inf". Typed
    /// levels below `floor_db` are refused rather than read as silence.
    Amplitude {
        floor_db: f32,
    },
    Hertz,
    Milliseconds,
    /// Stored in seconds, shown in milliseconds below one second.
    Seconds,
    /// Stored 0..1, shown 0..100 %.
    Percent,
    Ratio,
    Number {
        decimals: u8,
        unit: &'static str,
    },
}

impl ParamFormat {
    pub fn for_descriptor(descriptor: &ParamDescriptor) -> Self {
        match descriptor.unit {
            "dB" => Self::Decibels,
            "Hz" => Self::Hertz,
            "ms" => Self::Milliseconds,
            "s" => Self::Seconds,
            ":1" => Self::Ratio,
            "" if descriptor.min == 0.0 && descriptor.max == 1.0 => Self::Percent,
            unit => Self::Number { decimals: 2, unit },
        }
    }

    /// The value with its unit, as shown under a control.
    pub fn format(self, value: f32) -> String {
        match self {
            Self::Amplitude { .. } if value <= 0.0 || !value.is_finite() => "-inf dB".into(),
            Self::Hertz if value >= 1000.0 => format!("{:.2} kHz", value / 1000.0),
            Self::Amplitude { .. } | Self::Decibels | Self::Seconds | Self::Hertz => {
                format!("{} {}", self.edit_text(value), self.unit(value))
            }
            Self::Milliseconds => format!("{} ms", self.edit_text(value)),
            Self::Percent => format!("{} %", self.edit_text(value)),
            Self::Ratio => format!("{} :1", self.edit_text(value)),
            Self::Number { unit: "", .. } => self.edit_text(value),
            Self::Number { unit, .. } => format!("{} {unit}", self.edit_text(value)),
        }
    }

    /// The number alone, as it appears when typing starts.
    pub fn edit_text(self, value: f32) -> String {
        match self {
            Self::Amplitude { .. } if value <= 0.0 || !value.is_finite() => "-inf".into(),
            Self::Amplitude { .. } => format!("{:.1}", vibez_core::level::amplitude_to_db(value)),
            Self::Decibels => format!("{value:.1}"),
            Self::Hertz if value >= 1000.0 => format!("{:.2}k", value / 1000.0),
            Self::Hertz => format!("{value:.0}"),
            Self::Milliseconds => format!("{value:.1}"),
            Self::Seconds if value < 1.0 => format!("{:.0}", value * 1000.0),
            Self::Seconds => format!("{value:.2}"),
            Self::Percent => format!("{:.0}", value * 100.0),
            Self::Ratio => format!("{value:.1}"),
            Self::Number { decimals, .. } => format!("{value:.*}", decimals as usize),
        }
    }

    fn unit(self, value: f32) -> &'static str {
        match self {
            Self::Hertz => "Hz",
            Self::Seconds if value < 1.0 => "ms",
            Self::Seconds => "s",
            _ => "dB",
        }
    }

    /// The stored value typed text means, or `None` when it is not a
    /// number in `min..=max`. A unit may follow the number; a "k" suffix on
    /// frequencies means kHz, and times are milliseconds unless "s" follows.
    pub fn parse(self, text: &str, min: f32, max: f32) -> Option<f32> {
        let text = text.trim().to_ascii_lowercase();
        if matches!(self, Self::Amplitude { .. }) && matches!(text.as_str(), "-inf" | "-∞") {
            return Some(0.0);
        }
        let split = text
            .find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | '-' | '+')))
            .unwrap_or(text.len());
        let number = text[..split].parse::<f32>().ok()?;
        let suffix = text[split..].trim();
        if !number.is_finite() {
            return None;
        }
        let value = match self {
            Self::Amplitude { floor_db } => {
                let ceiling_db = vibez_core::level::amplitude_to_db(max);
                return (floor_db..=ceiling_db)
                    .contains(&number)
                    .then(|| vibez_core::level::db_to_amplitude(number).clamp(min, max));
            }
            Self::Hertz if suffix.starts_with('k') => number * 1000.0,
            Self::Seconds if suffix == "s" => number,
            Self::Seconds => number / 1000.0,
            Self::Percent => number / 100.0,
            _ => number,
        };
        (min..=max).contains(&value).then_some(value)
    }
}

/// The format a built-in effect parameter uses. Devices whose stored value
/// differs from how it reads add their parameter here.
pub fn param_format(
    effect_type: EffectType,
    index: usize,
    descriptor: &ParamDescriptor,
) -> ParamFormat {
    match (effect_type, index) {
        (EffectType::Gain, vibez_dsp::gain::param::GAIN) => ParamFormat::Amplitude {
            floor_db: crate::domains::gain::MIN_GAIN_DB,
        },
        _ => ParamFormat::for_descriptor(descriptor),
    }
}

#[derive(Debug, Clone)]
pub enum EntryMsg {
    Begin {
        track_id: TrackId,
        effect_id: EffectId,
        index: usize,
        text: String,
    },
    Edit(String),
    Submit,
}

/// The one parameter being typed into.
#[derive(Debug, Clone, PartialEq)]
pub struct ValueEntry {
    pub track_id: TrackId,
    pub effect_id: EffectId,
    pub index: usize,
    pub text: String,
    pub invalid: bool,
}

impl ValueEntry {
    pub fn is_for(&self, effect_id: EffectId, index: usize) -> bool {
        self.effect_id == effect_id && self.index == index
    }
}

#[derive(Debug, PartialEq)]
pub enum EntryAction {
    None,
    SetParameter {
        track_id: TrackId,
        effect_id: EffectId,
        index: usize,
        value: f32,
    },
}

/// Applies an entry message. `parse` reads the open entry's text for its
/// parameter; `current` is that parameter's stored value.
pub fn update(
    entry: &mut Option<ValueEntry>,
    message: EntryMsg,
    parse: impl FnOnce(&ValueEntry) -> Option<f32>,
    current: Option<f32>,
) -> EntryAction {
    match message {
        EntryMsg::Begin {
            track_id,
            effect_id,
            index,
            text,
        } => {
            *entry = Some(ValueEntry {
                track_id,
                effect_id,
                index,
                text,
                invalid: false,
            });
        }
        EntryMsg::Edit(text) => {
            if let Some(open) = entry {
                open.text = text;
                open.invalid = false;
            }
        }
        EntryMsg::Submit => {
            let Some(open) = entry.as_mut() else {
                return EntryAction::None;
            };
            let Some(value) = parse(open) else {
                open.invalid = true;
                return EntryAction::None;
            };
            let Some(open) = entry.take() else {
                return EntryAction::None;
            };
            if current != Some(value) {
                return EntryAction::SetParameter {
                    track_id: open.track_id,
                    effect_id: open.effect_id,
                    index: open.index,
                    value,
                };
            }
        }
    }
    EntryAction::None
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMPLITUDE: ParamFormat = ParamFormat::Amplitude { floor_db: -60.0 };

    fn descriptor(min: f32, max: f32, unit: &'static str) -> ParamDescriptor {
        ParamDescriptor {
            name: "Test",
            min,
            max,
            default: min,
            unit,
        }
    }

    #[test]
    fn values_read_the_way_a_producer_says_them() {
        assert_eq!(ParamFormat::Hertz.format(80.0), "80 Hz");
        assert_eq!(ParamFormat::Hertz.format(1200.0), "1.20 kHz");
        assert_eq!(ParamFormat::Decibels.format(-18.0), "-18.0 dB");
        assert_eq!(AMPLITUDE.format(1.0), "0.0 dB");
        assert_eq!(AMPLITUDE.format(0.0), "-inf dB");
        assert_eq!(ParamFormat::Seconds.format(0.12), "120 ms");
        assert_eq!(ParamFormat::Seconds.format(1.5), "1.50 s");
        assert_eq!(ParamFormat::Milliseconds.format(10.0), "10.0 ms");
        assert_eq!(ParamFormat::Percent.format(0.24), "24 %");
        assert_eq!(ParamFormat::Ratio.format(4.0), "4.0 :1");
    }

    #[test]
    fn typed_text_accepts_units_and_shorthand() {
        assert_eq!(
            ParamFormat::Hertz.parse("1.2k", 20.0, 20_000.0),
            Some(1200.0)
        );
        assert_eq!(
            ParamFormat::Hertz.parse("1.2 kHz", 20.0, 20_000.0),
            Some(1200.0)
        );
        assert_eq!(ParamFormat::Hertz.parse("440", 20.0, 20_000.0), Some(440.0));
        assert_eq!(ParamFormat::Decibels.parse("-6 dB", -60.0, 0.0), Some(-6.0));
        assert_eq!(ParamFormat::Percent.parse("24%", 0.0, 1.0), Some(0.24));
        assert_eq!(ParamFormat::Seconds.parse("120", 0.001, 5.0), Some(0.12));
        assert_eq!(ParamFormat::Seconds.parse("1.5 s", 0.001, 5.0), Some(1.5));
        assert_eq!(AMPLITUDE.parse("-inf", 0.0, 2.0), Some(0.0));
        let unity = AMPLITUDE.parse("0", 0.0, 2.0).unwrap();
        assert!((unity - 1.0).abs() < 1e-6);
    }

    #[test]
    fn typed_text_outside_the_range_or_not_a_number_is_refused() {
        assert_eq!(ParamFormat::Hertz.parse("30k", 20.0, 20_000.0), None);
        assert_eq!(ParamFormat::Decibels.parse("loud", -60.0, 0.0), None);
        assert_eq!(ParamFormat::Decibels.parse("", -60.0, 0.0), None);
        assert_eq!(AMPLITUDE.parse("+12", 0.0, 2.0), None);
        assert_eq!(AMPLITUDE.parse("-90", 0.0, 2.0), None);
    }

    #[test]
    fn edit_text_parses_back_to_the_value_it_shows() {
        for (format, value, min, max) in [
            (ParamFormat::Hertz, 1200.0, 20.0, 20_000.0),
            (ParamFormat::Decibels, -18.0, -60.0, 0.0),
            (ParamFormat::Seconds, 0.12, 0.001, 5.0),
            (ParamFormat::Percent, 0.24, 0.0, 1.0),
        ] {
            let parsed = format.parse(&format.edit_text(value), min, max).unwrap();
            assert!((parsed - value).abs() < 1e-4, "{format:?}");
        }
    }

    #[test]
    fn descriptors_choose_their_format_from_the_unit() {
        assert_eq!(
            ParamFormat::for_descriptor(&descriptor(20.0, 20_000.0, "Hz")),
            ParamFormat::Hertz
        );
        assert_eq!(
            ParamFormat::for_descriptor(&descriptor(0.0, 1.0, "")),
            ParamFormat::Percent
        );
        assert_eq!(
            ParamFormat::for_descriptor(&descriptor(1.0, 16.0, "bit")),
            ParamFormat::Number {
                decimals: 2,
                unit: "bit"
            }
        );
        assert_eq!(
            param_format(
                EffectType::Gain,
                vibez_dsp::gain::param::GAIN,
                &descriptor(0.0, 2.0, "")
            ),
            ParamFormat::Amplitude { floor_db: -60.0 }
        );
    }

    fn begin(entry: &mut Option<ValueEntry>, index: usize) -> (TrackId, EffectId) {
        let (track_id, effect_id) = (TrackId::new(), EffectId::new());
        update(
            entry,
            EntryMsg::Begin {
                track_id,
                effect_id,
                index,
                text: "1.0".into(),
            },
            |_| None,
            None,
        );
        (track_id, effect_id)
    }

    #[test]
    fn submitting_a_valid_entry_sets_the_parameter_and_closes_it() {
        let mut entry = None;
        let (track_id, effect_id) = begin(&mut entry, 3);
        update(&mut entry, EntryMsg::Edit("2.5".into()), |_| None, None);
        let action = update(
            &mut entry,
            EntryMsg::Submit,
            |open| open.text.parse().ok(),
            Some(1.0),
        );
        assert_eq!(
            action,
            EntryAction::SetParameter {
                track_id,
                effect_id,
                index: 3,
                value: 2.5
            }
        );
        assert!(entry.is_none());
    }

    #[test]
    fn an_invalid_entry_stays_open_and_marked_until_edited() {
        let mut entry = None;
        begin(&mut entry, 0);
        let action = update(&mut entry, EntryMsg::Submit, |_| None, Some(1.0));
        assert_eq!(action, EntryAction::None);
        assert!(entry.as_ref().unwrap().invalid);
        update(&mut entry, EntryMsg::Edit("2".into()), |_| None, None);
        assert!(!entry.as_ref().unwrap().invalid);
    }

    #[test]
    fn submitting_the_current_value_closes_without_an_edit() {
        let mut entry = None;
        begin(&mut entry, 0);
        let action = update(&mut entry, EntryMsg::Submit, |_| Some(1.0), Some(1.0));
        assert_eq!(action, EntryAction::None);
        assert!(entry.is_none());
    }

    #[test]
    fn beginning_another_entry_replaces_the_open_one() {
        let mut entry = None;
        begin(&mut entry, 0);
        let (_, effect_id) = begin(&mut entry, 2);
        assert!(entry.as_ref().unwrap().is_for(effect_id, 2));
    }
}
