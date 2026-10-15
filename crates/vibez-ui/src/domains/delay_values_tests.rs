//! Musical Delay field values preserve DSP units and reject ambiguous numeric edits.

use super::delay_values::{display, parse};
use vibez_dsp::delay::{param, DELAY_PARAMS};

#[test]
fn delay_values_parse_musical_units_and_display_stored_targets_consistently() {
    for index in [param::FEEDBACK, param::MIX, param::DUCK] {
        let descriptor = &DELAY_PARAMS[index];
        for text in ["35", "35%", " 35 % "] {
            assert!((parse(index, text, descriptor).unwrap() - 0.35).abs() <= 1e-6);
        }
        for text in ["NaN", "inf", "-1%", "101", "35%%", "35 percent", "35ms", ""] {
            assert!(parse(index, text, descriptor).is_none(), "{index}: {text:?}");
        }
        let shown = display(index, 0.35, descriptor);
        assert!((shown.parse::<f32>().unwrap() - 35.0).abs() <= 1e-5);
        assert!((parse(index, &shown, descriptor).unwrap() - 0.35).abs() <= 1e-6);
    }
    assert_eq!(parse(param::FEEDBACK, "95%", &DELAY_PARAMS[param::FEEDBACK]), Some(0.95));
    assert!(parse(param::FEEDBACK, "96%", &DELAY_PARAMS[param::FEEDBACK]).is_none());
    for index in [param::TIME_MS, param::RIGHT_TIME_MS] {
        let descriptor = &DELAY_PARAMS[index];
        for text in ["375", "375ms", " 375 MS ", ".375s", " .375 s "] {
            assert_eq!(parse(index, text, descriptor), Some(375.0), "{text:?}");
        }
        assert_eq!(parse(index, "2 s", descriptor), Some(2000.0));
        for text in [
            "NaN", "Infinity", "0", "-1", ".0005s", "2.001s", "2001ms",
            "375Hz", "375msms", "1s375ms", "1,5s", "1 / 8", "",
        ] {
            assert!(parse(index, text, descriptor).is_none(), "{index}: {text:?}");
        }
        let shown = display(index, 375.0, descriptor);
        assert_eq!(shown.parse::<f32>().unwrap(), 375.0);
        assert_eq!(parse(index, &shown, descriptor), Some(375.0));
    }
    for (index, value, inputs) in [
        (param::FEEDBACK_HP_HZ, 120.0, ["120", "120 Hz", ".12k", " .12 kHz "]),
        (param::FEEDBACK_LP_HZ, 8000.0, ["8000", "8000 Hz", "8k", " 8 KHZ "]),
    ] {
        let descriptor = &DELAY_PARAMS[index];
        for text in inputs {
            assert_eq!(parse(index, text, descriptor), Some(value), "{text:?}");
        }
        for text in ["NaNHz", "inf", "8kk", "120HzHz", "120ms", ""] {
            assert!(parse(index, text, descriptor).is_none(), "{index}: {text:?}");
        }
        assert_eq!(parse(index, &display(index, value, descriptor), descriptor), Some(value));
    }
    assert!(parse(param::FEEDBACK_HP_HZ, "2k", &DELAY_PARAMS[param::FEEDBACK_HP_HZ]).is_none());
    assert!(parse(param::FEEDBACK_LP_HZ, ".5k", &DELAY_PARAMS[param::FEEDBACK_LP_HZ]).is_none());
    for index in [
        param::SYNC, param::LEFT_DIVISION, param::LEFT_MODIFIER, param::ROUTING,
        param::LINK, param::RIGHT_DIVISION, param::RIGHT_MODIFIER,
        param::FEEDBACK_HP_ENABLED, param::FEEDBACK_LP_ENABLED, param::TIME_CHANGE,
    ] {
        let descriptor = &DELAY_PARAMS[index];
        assert!(parse(index, "0.5", descriptor).is_none(), "discrete field {index}");
        assert_eq!(parse(index, &descriptor.max.to_string(), descriptor), Some(descriptor.max));
    }
    assert!(parse(param::COUNT, "375", &DELAY_PARAMS[param::TIME_MS]).is_none());
}
