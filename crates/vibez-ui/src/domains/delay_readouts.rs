//! Formats current Delay timing and effective feedback cutoffs for native control tooltips.

use vibez_core::delay_state::DelayState;
use vibez_dsp::delay::param;

pub fn bank_tooltip(snapshot: Option<&DelayState>) -> String {
    let Some(state) = snapshot.filter(|state| state.sample_rate > 0) else {
        return "Actual timing, feedback cutoffs and Duck attenuation unavailable".into();
    };
    let rate = f64::from(state.sample_rate);
    let milliseconds = 1000.0 / rate;
    let timing: [String; 2] = std::array::from_fn(|index| {
        let name = ["L", "R"][index];
        let line = state.lines[index];
        let remaining = line.remaining_frames as f64 / rate;
        if line.moving && !line.gliding {
            format!(
                "{name} crossfade: taps {:.2}/{:.2} ms; {:.0}% blended; target {:.2} ms; {remaining:.3} s remaining.",
                line.old_samples * milliseconds, line.destination_samples * milliseconds,
                line.progress * 100.0, line.target_samples * milliseconds,
            )
        } else {
            let mode = if line.gliding { "pitch glide" } else { "settled" };
            format!(
                "{name} {mode}: applied {:.2} ms; target {:.2} ms; {remaining:.3} s remaining.",
                line.old_samples * milliseconds, line.target_samples * milliseconds,
            )
        }
    });
    let filters: [String; 2] = std::array::from_fn(|index| {
        let name = ["HP", "LP"][index];
        let parameter = [param::FEEDBACK_HP_HZ, param::FEEDBACK_LP_HZ][index];
        let stored = state.targets[parameter];
        let filter = state.filters[index];
        let limited = if f64::from(stored) > 0.45 * rate { " (sample-rate limited)" } else { "" };
        let moving = if filter.moving { "; moving" } else { "" };
        format!(
            "{name} stored {stored:.2} Hz; effective {:.2} Hz{limited}; enable {:.0}%{moving}.",
            filter.cutoff_hz, filter.enabled * 100.0,
        )
    });
    let linking = if state.targets[param::LINK] == 1.0 {
        "Linked timing uses Left; stored Right settings are retained."
    } else { "Left and Right timing are independent." };
    format!(
        "Applied BPM {:.2}.\n{}\n{}\nWet-only Duck attenuation {:.1} dB.\n{linking}",
        state.bpm, timing.join("\n"), filters.join("\n"), state.duck_attenuation_db,
    )
}
