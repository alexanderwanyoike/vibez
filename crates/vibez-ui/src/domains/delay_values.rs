//! Delay field policies translate musical units without changing stored parameter values.

use super::device_controls::{display_value, ValuePolicy};
use vibez_core::effect::ParamDescriptor;
use vibez_dsp::delay::param;

pub fn policy(index: usize) -> ValuePolicy {
    let unit = match index {
        param::FEEDBACK | param::MIX | param::DUCK => "%",
        param::TIME_MS | param::RIGHT_TIME_MS => "ms",
        param::FEEDBACK_HP_HZ | param::FEEDBACK_LP_HZ => "Hz",
        _ => "",
    };
    ValuePolicy {
        label: match index {
            param::RIGHT_TIME_MS => Some("R Time"),
            param::FEEDBACK_HP_HZ | param::FEEDBACK_LP_HZ => Some("Cutoff"),
            _ => None,
        },
        display,
        parse,
        unit,
    }
}

pub fn parse(index: usize, text: &str, descriptor: &ParamDescriptor) -> Option<f32> {
    if index >= param::COUNT {
        return None;
    }
    let text = text.trim().to_ascii_lowercase();
    let value = match policy(index).unit {
        "%" => {
            text.strip_suffix('%')
                .unwrap_or(&text)
                .trim()
                .parse::<f32>()
                .ok()?
                / 100.0
        }
        "ms" => {
            let (number, multiplier) = if let Some(number) = text.strip_suffix("ms") {
                (number, 1.0)
            } else if let Some(number) = text.strip_suffix('s') {
                (number, 1000.0)
            } else {
                (text.as_str(), 1.0)
            };
            number.trim().parse::<f32>().ok()? * multiplier
        }
        "Hz" => {
            let number = text.strip_suffix("hz").unwrap_or(&text).trim();
            let (number, multiplier) = match number.strip_suffix('k') {
                Some(number) => (number, 1000.0),
                None => (number, 1.0),
            };
            number.trim().parse::<f32>().ok()? * multiplier
        }
        _ => text.parse::<f32>().ok()?,
    };
    let discrete = matches!(
        index,
        param::SYNC
            | param::LEFT_DIVISION
            | param::LEFT_MODIFIER
            | param::ROUTING
            | param::LINK
            | param::RIGHT_DIVISION
            | param::RIGHT_MODIFIER
            | param::FEEDBACK_HP_ENABLED
            | param::FEEDBACK_LP_ENABLED
            | param::TIME_CHANGE
    );
    (value.is_finite()
        && (descriptor.min..=descriptor.max).contains(&value)
        && (!discrete || value.fract() == 0.0))
        .then_some(value)
}

pub fn display(index: usize, value: f32, descriptor: &ParamDescriptor) -> String {
    if index >= param::COUNT || !value.is_finite() {
        return "N/A".into();
    }
    // The shared field renders its unit separately, so only the numeric text is returned.
    display_value(
        value,
        &ParamDescriptor {
            unit: policy(index).unit,
            ..*descriptor
        },
    )
}
