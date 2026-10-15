//! Renders bounded static wet references with the production Delay kernel off the UI thread.

use std::sync::atomic::{AtomicBool, Ordering};
use vibez_core::{
    audio_context::DeviceAudioContext,
    constants::{MAX_BPM, MIN_BPM},
    delay_state::DelayState,
};
use vibez_dsp::{
    delay::{param, DelayEffect, DELAY_PARAMS},
    effect::AudioEffect,
};

pub const BINS: usize = 512;
const CHUNK: usize = 1024;

#[derive(Clone, Copy, Debug)]
pub struct PeakBin {
    pub left: f32,
    pub right: f32,
    pub left_frame: u32,
    pub right_frame: u32,
}

#[derive(Clone, Debug)]
pub struct DelayResponse {
    pub bins: [PeakBin; BINS],
    pub frames: u64,
    pub sample_rate: u32,
}

fn distance(state: &DelayState, right: bool) -> f64 {
    let p = &state.targets;
    let (free, division, modifier) = if right && p[param::LINK] == 0.0 {
        (
            param::RIGHT_TIME_MS,
            param::RIGHT_DIVISION,
            param::RIGHT_MODIFIER,
        )
    } else {
        (param::TIME_MS, param::LEFT_DIVISION, param::LEFT_MODIFIER)
    };
    if p[param::SYNC] == 0.0 {
        f64::from(p[free]) * f64::from(state.sample_rate) / 1000.0
    } else {
        (60.0 / state.bpm)
            * (4.0 / [64.0, 32.0, 16.0, 8.0, 4.0, 2.0, 1.0][p[division] as usize])
            * [1.0, 1.5, 2.0 / 3.0][p[modifier] as usize]
            * f64::from(state.sample_rate)
    }
}

fn window(state: &DelayState) -> Option<u64> {
    let rate = u64::from(state.sample_rate);
    if state.reference_moving
        || state.processed_frames == 0
        || rate < 1000
        || (18 * rate + 2) * 8 > 64 * 1024 * 1024
        || !state.bpm.is_finite()
        || !(MIN_BPM..=MAX_BPM).contains(&state.bpm)
        || !state.mix.is_finite()
        || !(0.0..=1.0).contains(&state.mix)
        || !state.duck_attenuation_db.is_finite()
        || !(0.0..=24.0).contains(&state.duck_attenuation_db)
    {
        return None;
    }
    for (index, (&value, descriptor)) in state.targets.iter().zip(DELAY_PARAMS).enumerate() {
        if !value.is_finite()
            || !(descriptor.min..=descriptor.max).contains(&value)
            || (matches!(index, 3..=7 | 9..=11 | 13 | 16) && value.fract() != 0.0)
        {
            return None;
        }
    }
    if state.feedback != state.targets[param::FEEDBACK]
        || state.routing != state.targets[param::ROUTING]
    {
        return None;
    }
    for (filter, enable, cutoff) in [
        (
            state.filters[0],
            param::FEEDBACK_HP_ENABLED,
            param::FEEDBACK_HP_HZ,
        ),
        (
            state.filters[1],
            param::FEEDBACK_LP_ENABLED,
            param::FEEDBACK_LP_HZ,
        ),
    ] {
        let limit = 0.45 * rate as f64;
        let minimum = f64::from(DELAY_PARAMS[cutoff].min).min(limit);
        if filter.enabled != state.targets[enable]
            || !filter.cutoff_hz.is_finite()
            || !(minimum..=limit).contains(&filter.cutoff_hz)
            || (filter.enabled != 0.0
                && (filter.moving
                    || filter.cutoff_hz != f64::from(state.targets[cutoff]).min(limit)))
        {
            return None;
        }
    }
    let mut longest = 0.0_f64;
    for (index, line) in state.lines.iter().enumerate() {
        let expected = distance(state, index == 1);
        if !expected.is_finite()
            || expected <= 0.0
            || expected > 18.0 * rate as f64
            || line.moving
            || line.gliding
            || line.pending_samples.is_some()
            || line.remaining_frames != 0
            || line.progress != 0.0
            || line.old_samples != expected
            || line.destination_samples != expected
            || line.target_samples != expected
        {
            return None;
        }
        longest = longest.max(expected);
    }
    let endpoint = (4.0 * longest).ceil();
    if endpoint > u32::MAX as f64 {
        return None;
    }
    (endpoint as u64).checked_add(1)
}

pub fn render_reference(state: &DelayState, cancel: &AtomicBool) -> Option<DelayResponse> {
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    let frames = window(state)?;
    let mut effect = DelayEffect::new(state.sample_rate as f32);
    if cancel.load(Ordering::Acquire) || !effect.processing_configuration_valid() {
        return None;
    }
    for (index, &target) in state.targets.iter().enumerate() {
        let value = match index {
            param::MIX => 1.0,
            param::DUCK => 0.0,
            _ => target,
        };
        if !effect.set_param(index, value) {
            return None;
        }
    }
    effect.set_audio_context(DeviceAudioContext {
        musical_sample: 0,
        continuous_sample: 0,
        sample_rate: state.sample_rate,
        bpm: state.bpm,
        playing: false,
    });
    effect.reset();
    let mut bins = [PeakBin {
        left: 0.0,
        right: 0.0,
        left_frame: u32::MAX,
        right_frame: u32::MAX,
    }; BINS];
    for (index, bin) in bins.iter_mut().enumerate() {
        let start = index as u64 * frames / BINS as u64;
        let end = (index + 1) as u64 * frames / BINS as u64;
        if start < end {
            bin.left_frame = start as u32;
            bin.right_frame = start as u32;
        }
    }
    let mut scratch = [0.0; 2 * CHUNK];
    let mut processed = 0_u64;
    while processed < frames {
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        let count = (frames - processed).min(CHUNK as u64) as usize;
        scratch[..2 * count].fill(0.0);
        if processed == 0 {
            scratch[0] = 1.0;
        }
        effect.process(&mut scratch[..2 * count], 2);
        if cancel.load(Ordering::Acquire) {
            return None;
        }
        for (offset, pair) in scratch[..2 * count].chunks_exact(2).enumerate() {
            if !pair.iter().all(|sample| sample.is_finite()) {
                return None;
            }
            let frame = processed + offset as u64;
            // Inverting the floor partition skips empty bins without shifting an endpoint.
            let index = (((frame + 1) * BINS as u64 - 1) / frames) as usize;
            let bin = &mut bins[index];
            if pair[0].abs() > bin.left {
                bin.left = pair[0].abs();
                bin.left_frame = frame as u32;
            }
            if pair[1].abs() > bin.right {
                bin.right = pair[1].abs();
                bin.right_frame = frame as u32;
            }
        }
        processed += count as u64;
    }
    if cancel.load(Ordering::Acquire) {
        return None;
    }
    Some(DelayResponse {
        bins,
        frames,
        sample_rate: state.sample_rate,
    })
}
