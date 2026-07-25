//! Offline rendering (bounce / resample).
//!
//! Builds a fresh set of [`EngineTrack`]s from a project snapshot, drives them
//! block-by-block without an audio I/O stream, and returns a
//! [`DecodedAudio`] buffer. The same mixer and instrument paths that run on
//! the audio thread are used, so what you bounce matches what you hear.
//!
//! Third-party plugin instances are prepared by the UI (their main-thread
//! initialization cannot happen in this crate) and supplied to
//! [`render_offline_with_plugins`]. A declared plugin is never silently
//! substituted or skipped by that strict export path.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use vibez_core::audio_buffer::DecodedAudio;
use vibez_core::id::{ClipId, EffectId, TrackId};
use vibez_core::midi::NoteClipInfo;
use vibez_core::time::TempoMap;
use vibez_core::track::{ClipInfo, InstrumentStateInfo, TrackInfo};
use vibez_dsp::factory::create_effect_with_params;
use vibez_instruments::create_instrument;

use crate::mixer::{
    any_solo, equal_power_pan, EffectSlot, EngineClip, EngineNoteClip, EngineTrack,
    InstrumentRenderContext,
};

/// What to render offline.
#[derive(Debug, Clone, Copy)]
pub enum BounceMode {
    /// Sum of all non-muted / soloed tracks. Mute and solo rules mirror live
    /// playback.
    Master,
    /// Render one track post-effects with its gain/pan. Mute/solo on the
    /// target are ignored so you always hear the thing you asked for.
    Track(TrackId),
    /// Render a single clip on one track. Other clips on the same track and
    /// all other tracks are excluded.
    Clip {
        track_id: TrackId,
        clip_id: ClipId,
        is_note_clip: bool,
    },
}

/// Snapshot of the parts of a project needed to render offline, plus the
/// decoded audio for every asset referenced by the snapshot. The live Audition
/// Bus is intentionally absent, so exports, stems, and resampling cannot capture
/// Browser playback.
pub struct BounceRequest {
    pub tracks: Vec<TrackInfo>,
    /// Master bus (gain + effect chain), applied to the summed mix
    /// in [`BounceMode::Master`] renders.
    pub master: Option<TrackInfo>,
    /// Return buses; fed by track sends in [`BounceMode::Master`].
    pub buses: Vec<TrackInfo>,
    pub audio_clips: Vec<ClipInfo>,
    pub note_clips: Vec<NoteClipInfo>,
    pub clip_audio: HashMap<ClipId, Arc<DecodedAudio>>,
    pub sampler_audio: HashMap<TrackId, (Arc<DecodedAudio>, String)>,
    pub drum_pad_audio: HashMap<(TrackId, usize), (Arc<DecodedAudio>, String)>,
    pub mode: BounceMode,
    /// Render window in absolute samples: `[start, end)` at `sample_rate`.
    pub range_samples: (u64, u64),
    pub bpm: f64,
    pub sample_rate: u32,
    pub swing: vibez_core::perform::SwingAmount,
}

pub struct BounceResult {
    pub audio: DecodedAudio,
    pub warnings: Vec<String>,
}

/// Isolated third-party devices prepared for one offline render.
///
/// Keys are project ids, so each declared slot consumes exactly the instance
/// prepared for it. Missing devices are fatal in the strict export path.
#[derive(Default)]
pub struct OfflinePlugins {
    pub instruments: HashMap<TrackId, Box<dyn vibez_instruments::Instrument>>,
    pub effects: HashMap<EffectId, Box<dyn vibez_dsp::effect::AudioEffect>>,
}

const BLOCK_FRAMES: usize = 512;
const CHANNELS: usize = 2;

/// Render the request and return interleaved stereo [`DecodedAudio`] at the
/// project's sample rate.
pub fn render_offline(req: &BounceRequest) -> BounceResult {
    render_offline_inner(req, None, |_| {})
        .expect("the compatibility renderer cannot fail without strict plugin preparation")
}

/// Strict production renderer used by project export.
///
/// Every plugin declared by the snapshot must have a matching isolated
/// instance in `plugins`. `progress` receives monotonic percentages from
/// 0 through 100.
pub fn render_offline_with_plugins(
    req: &BounceRequest,
    plugins: &mut OfflinePlugins,
    progress: impl FnMut(u8),
) -> Result<BounceResult, String> {
    validate_offline_plugins(req, plugins)?;
    render_offline_inner(req, Some(plugins), progress)
}

fn render_offline_inner(
    req: &BounceRequest,
    mut plugins: Option<&mut OfflinePlugins>,
    mut progress: impl FnMut(u8),
) -> Result<BounceResult, String> {
    let mut warnings = Vec::new();
    progress(0);
    let sr_f32 = req.sample_rate as f32;
    let tempo = TempoMap::new(req.bpm, req.sample_rate);

    let mut tracks: Vec<EngineTrack> = Vec::with_capacity(req.tracks.len());

    for track_info in &req.tracks {
        if !track_is_active_for_mode(track_info.id, req.mode) {
            continue;
        }

        let mut engine = EngineTrack::new(track_info.id);
        engine.gain = track_info.gain;
        engine.pan = track_info.pan;
        engine.swing_offset = track_info.swing_offset;
        engine.sends = track_info.sends.clone();
        match req.mode {
            BounceMode::Master => {
                engine.set_manual_mute(track_info.mute, true);
                engine.solo = track_info.solo;
            }
            _ => {
                engine.set_manual_mute(false, true);
                engine.solo = false;
            }
        }

        if let Some(device) = &track_info.plugin_instrument {
            let instrument = plugins
                .as_deref_mut()
                .and_then(|prepared| prepared.instruments.remove(&track_info.id));
            match instrument {
                Some(instrument) => engine.instrument = Some(instrument),
                None if plugins.is_some() => {
                    return Err(format!(
                        "Track '{}' requires {} plugin instrument '{}', but it was not prepared",
                        track_info.name,
                        device.format.to_uppercase(),
                        device.name
                    ));
                }
                None => warnings.push(format!(
                    "Track '{}' plugin instrument is unavailable in this render",
                    track_info.name
                )),
            }
        } else if let Some(kind) = track_info.instrument {
            let mut instrument = create_instrument(kind, sr_f32);
            if let Some(state) = &track_info.native_instrument {
                match state {
                    InstrumentStateInfo::SubtractiveSynth { params } => {
                        for (i, v) in params.iter().enumerate() {
                            instrument.set_param(i, *v);
                        }
                    }
                    InstrumentStateInfo::Sampler { params, .. } => {
                        for (i, v) in params.iter().enumerate() {
                            instrument.set_param(i, *v);
                        }
                        if let Some((audio, name)) = req.sampler_audio.get(&track_info.id) {
                            instrument.load_sample(Arc::clone(audio), name.clone());
                        }
                    }
                    InstrumentStateInfo::DrumRack { pads } => {
                        for (idx, pad) in pads.iter().enumerate() {
                            instrument.set_drum_pad_state(idx, pad.clone());
                        }
                        for (idx, _) in pads.iter().enumerate() {
                            if let Some((audio, name)) =
                                req.drum_pad_audio.get(&(track_info.id, idx))
                            {
                                instrument.load_drum_pad_sample(
                                    idx,
                                    Arc::clone(audio),
                                    name.clone(),
                                );
                            }
                        }
                    }
                }
            }
            engine.instrument = Some(instrument);
        } else if track_info.kind.is_midi() {
            warnings.push(format!(
                "Track '{}' has no native instrument; any plugin instrument will not render",
                track_info.name
            ));
        }

        for info in &track_info.effects {
            let fx = if let Some(device) = &info.plugin {
                match plugins
                    .as_deref_mut()
                    .and_then(|prepared| prepared.effects.remove(&info.id))
                {
                    Some(effect) => effect,
                    None if plugins.is_some() => {
                        return Err(format!(
                            "Track '{}' requires {} effect '{}', but it was not prepared",
                            track_info.name,
                            device.format.to_uppercase(),
                            device.name
                        ));
                    }
                    None => {
                        warnings.push(format!(
                            "Track '{}' plugin effect '{}' is unavailable in this render",
                            track_info.name, device.name
                        ));
                        continue;
                    }
                }
            } else {
                create_effect_with_params(info.effect_type, sr_f32, &info.params)
            };
            engine.effects.push(EffectSlot {
                id: info.id,
                effect: fx,
                bypass: info.bypass,
            });
        }

        for clip in req
            .audio_clips
            .iter()
            .filter(|c| c.track_id == track_info.id)
        {
            if !clip_included_for_mode(clip.id, req.mode, false) {
                continue;
            }
            match req.clip_audio.get(&clip.id) {
                Some(audio) => engine.playback_source.clips.push(EngineClip {
                    id: clip.id,
                    audio: Arc::clone(audio),
                    position: clip.position,
                    source_offset: clip.source_offset,
                    duration: clip.duration,
                    loop_enabled: clip.loop_enabled,
                    loop_start: clip.loop_start,
                    loop_end: clip.loop_end,
                }),
                None => warnings.push(format!("Clip '{}' audio missing, skipped", clip.name)),
            }
        }

        for nc in req
            .note_clips
            .iter()
            .filter(|c| c.track_id == track_info.id)
        {
            if !clip_included_for_mode(nc.id, req.mode, true) {
                continue;
            }
            engine.playback_source.note_clips.push(EngineNoteClip::new(
                nc.id,
                nc.position_beats,
                nc.duration_beats,
                nc.notes.clone(),
                nc.loop_enabled,
                nc.loop_start_beats,
                nc.loop_end_beats,
                nc.groove_grid,
            ));
        }

        tracks.push(engine);
    }

    let (start, end) = req.range_samples;
    let total_frames = end.saturating_sub(start) as usize;
    let has_track_solo = matches!(req.mode, BounceMode::Master) && any_solo(&tracks);

    // Return buses: rebuilt like live channels, fed from track sends
    // per block. Only master-mode renders route through them.
    let mut buses: Vec<EngineTrack> = Vec::new();
    if matches!(req.mode, BounceMode::Master) {
        for bus_info in &req.buses {
            let mut bus = EngineTrack::new(bus_info.id);
            bus.gain = bus_info.gain;
            bus.pan = bus_info.pan;
            bus.mute = bus_info.mute;
            bus.solo = bus_info.solo;
            for info in &bus_info.effects {
                let effect = if let Some(device) = &info.plugin {
                    match plugins
                        .as_deref_mut()
                        .and_then(|prepared| prepared.effects.remove(&info.id))
                    {
                        Some(effect) => effect,
                        None if plugins.is_some() => {
                            return Err(format!(
                                "Bus '{}' requires {} effect '{}', but it was not prepared",
                                bus_info.name,
                                device.format.to_uppercase(),
                                device.name
                            ));
                        }
                        None => {
                            warnings.push(format!(
                                "Bus '{}' plugin effect '{}' is unavailable in this render",
                                bus_info.name, device.name
                            ));
                            continue;
                        }
                    }
                } else {
                    create_effect_with_params(info.effect_type, sr_f32, &info.params)
                };
                bus.effects.push(EffectSlot {
                    id: info.id,
                    effect,
                    bypass: info.bypass,
                });
            }
            buses.push(bus);
        }
    }
    let has_bus_solo = any_solo(&buses);

    // Master bus chain + gain, applied to the summed mix so the
    // export matches live playback. Only master-mode renders route
    // through it (single-track/clip bounces are pre-master stems).
    let mut master_fx: Vec<EffectSlot> = Vec::new();
    let mut master_gain = 1.0f32;
    if matches!(req.mode, BounceMode::Master) {
        if let Some(info) = &req.master {
            master_gain = info.gain;
            for fx_info in &info.effects {
                let effect = if let Some(device) = &fx_info.plugin {
                    match plugins
                        .as_deref_mut()
                        .and_then(|prepared| prepared.effects.remove(&fx_info.id))
                    {
                        Some(effect) => effect,
                        None if plugins.is_some() => {
                            return Err(format!(
                                "Master requires {} effect '{}', but it was not prepared",
                                device.format.to_uppercase(),
                                device.name
                            ));
                        }
                        None => {
                            warnings.push(format!(
                                "Master plugin effect '{}' is unavailable in this render",
                                device.name
                            ));
                            continue;
                        }
                    }
                } else {
                    create_effect_with_params(fx_info.effect_type, sr_f32, &fx_info.params)
                };
                master_fx.push(EffectSlot {
                    id: fx_info.id,
                    effect,
                    bypass: fx_info.bypass,
                });
            }
        }
    }

    let mut out_l = Vec::with_capacity(total_frames);
    let mut out_r = Vec::with_capacity(total_frames);

    let mut master_scratch = vec![0.0f32; BLOCK_FRAMES * CHANNELS];

    let mut rendered = 0usize;
    while rendered < total_frames {
        let block = (total_frames - rendered).min(BLOCK_FRAMES);
        let pos = start + rendered as u64;
        let scratch = &mut master_scratch[..block * CHANNELS];
        scratch.iter_mut().for_each(|s| *s = 0.0);

        for bus in buses.iter_mut() {
            bus.clear_buffer(block, CHANNELS);
        }

        for track in tracks.iter_mut() {
            if matches!(req.mode, BounceMode::Master)
                && has_track_solo
                && !track.solo
                && !has_bus_solo
            {
                continue;
            }

            let beat = pos as f64 / tempo.samples_per_beat();
            let (auto_gain, auto_pan) = track.apply_automation(beat);
            let produced = if track.instrument.is_some() {
                track.render_instrument(
                    InstrumentRenderContext {
                        pos,
                        repeat_pos: pos,
                        frames: block,
                        channels: CHANNELS,
                        tempo_map: &tempo,
                        project_swing: req.swing,
                    },
                    &mut |_| {},
                )
            } else {
                // Offline bounce never loops the arrangement — it
                // walks the timeline linearly from `range.0` to
                // `range.1`, so pass `None`.
                track.render(pos, block, CHANNELS, None)
            };
            if !produced {
                continue;
            }
            track.process_effects(block, CHANNELS);
            if matches!(req.mode, BounceMode::Master) {
                track.apply_mute_envelope(pos, block, CHANNELS, tempo.samples_per_beat());
            }

            let (pan_l, pan_r) = equal_power_pan(auto_pan.unwrap_or(track.pan));
            let gain = auto_gain.unwrap_or(track.gain);
            let dry_audible = (!has_track_solo && !has_bus_solo) || track.solo;
            for frame in 0..block {
                let idx = frame * CHANNELS;
                if dry_audible {
                    scratch[idx] += track.mix_buffer[idx] * gain * pan_l;
                    scratch[idx + 1] += track.mix_buffer[idx + 1] * gain * pan_r;
                }
            }
            for (bus_id, amount) in &track.sends {
                if *amount <= 0.0005 {
                    continue;
                }
                if let Some(bus) = buses.iter_mut().find(|b| b.id == *bus_id) {
                    for frame in 0..block {
                        let idx = frame * CHANNELS;
                        bus.mix_buffer[idx] += track.mix_buffer[idx] * gain * pan_l * amount;
                        bus.mix_buffer[idx + 1] +=
                            track.mix_buffer[idx + 1] * gain * pan_r * amount;
                    }
                }
            }
        }

        for bus in buses.iter_mut() {
            let buf = block * CHANNELS;
            for slot in &mut bus.effects {
                if !slot.bypass {
                    slot.effect.process(&mut bus.mix_buffer[..buf], CHANNELS);
                }
            }
            if bus.mute || (has_bus_solo && !bus.solo) {
                continue;
            }
            let (pan_l, pan_r) = crate::mixer::balance_pan(bus.pan);
            let gain = bus.gain;
            for frame in 0..block {
                let idx = frame * CHANNELS;
                scratch[idx] += bus.mix_buffer[idx] * gain * pan_l;
                scratch[idx + 1] += bus.mix_buffer[idx + 1] * gain * pan_r;
            }
        }

        for slot in &mut master_fx {
            if !slot.bypass {
                slot.effect.process(scratch, CHANNELS);
            }
        }
        if (master_gain - 1.0).abs() > f32::EPSILON {
            scratch.iter_mut().for_each(|s| *s *= master_gain);
        }

        for frame in 0..block {
            let idx = frame * CHANNELS;
            out_l.push(scratch[idx]);
            out_r.push(scratch[idx + 1]);
        }

        rendered += block;
        let percent = if total_frames == 0 {
            100
        } else {
            ((rendered as u128 * 100) / total_frames as u128).min(100) as u8
        };
        progress(percent);
    }

    if let Some(prepared) = plugins {
        return_offline_plugins(req, prepared, &mut tracks, &mut buses, &mut master_fx);
    }
    progress(100);
    Ok(BounceResult {
        audio: DecodedAudio {
            channels: vec![out_l, out_r],
            sample_rate: req.sample_rate,
        },
        warnings,
    })
}

fn validate_offline_plugins(req: &BounceRequest, plugins: &OfflinePlugins) -> Result<(), String> {
    for track in &req.tracks {
        if !track_is_active_for_mode(track.id, req.mode) {
            continue;
        }
        if let Some(device) = &track.plugin_instrument {
            if !plugins.instruments.contains_key(&track.id) {
                return Err(format!(
                    "Track '{}' requires {} plugin instrument '{}', but it was not prepared",
                    track.name,
                    device.format.to_uppercase(),
                    device.name
                ));
            }
        }
        for effect in &track.effects {
            if let Some(device) = &effect.plugin {
                if !plugins.effects.contains_key(&effect.id) {
                    return Err(format!(
                        "Track '{}' requires {} effect '{}', but it was not prepared",
                        track.name,
                        device.format.to_uppercase(),
                        device.name
                    ));
                }
            }
        }
    }
    if matches!(req.mode, BounceMode::Master) {
        for bus in &req.buses {
            for effect in &bus.effects {
                if let Some(device) = &effect.plugin {
                    if !plugins.effects.contains_key(&effect.id) {
                        return Err(format!(
                            "Bus '{}' requires {} effect '{}', but it was not prepared",
                            bus.name,
                            device.format.to_uppercase(),
                            device.name
                        ));
                    }
                }
            }
        }
        if let Some(master) = &req.master {
            for effect in &master.effects {
                if let Some(device) = &effect.plugin {
                    if !plugins.effects.contains_key(&effect.id) {
                        return Err(format!(
                            "Master requires {} effect '{}', but it was not prepared",
                            device.format.to_uppercase(),
                            device.name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn return_offline_plugins(
    req: &BounceRequest,
    prepared: &mut OfflinePlugins,
    tracks: &mut [EngineTrack],
    buses: &mut [EngineTrack],
    master_fx: &mut Vec<EffectSlot>,
) {
    let plugin_effect_ids: HashSet<EffectId> = req
        .tracks
        .iter()
        .chain(req.buses.iter())
        .chain(req.master.iter())
        .flat_map(|track| &track.effects)
        .filter(|effect| effect.plugin.is_some())
        .map(|effect| effect.id)
        .collect();

    for track in tracks {
        if req
            .tracks
            .iter()
            .find(|info| info.id == track.id)
            .is_some_and(|info| info.plugin_instrument.is_some())
        {
            if let Some(mut instrument) = track.instrument.take() {
                instrument.finish_offline_processing();
                prepared.instruments.insert(track.id, instrument);
            }
        }
        return_plugin_effects(&plugin_effect_ids, prepared, &mut track.effects);
    }
    for bus in buses {
        return_plugin_effects(&plugin_effect_ids, prepared, &mut bus.effects);
    }
    return_plugin_effects(&plugin_effect_ids, prepared, master_fx);
}

fn return_plugin_effects(
    plugin_effect_ids: &HashSet<EffectId>,
    prepared: &mut OfflinePlugins,
    slots: &mut Vec<EffectSlot>,
) {
    let mut index = 0;
    while index < slots.len() {
        if plugin_effect_ids.contains(&slots[index].id) {
            let mut slot = slots.remove(index);
            slot.effect.finish_offline_processing();
            prepared.effects.insert(slot.id, slot.effect);
        } else {
            index += 1;
        }
    }
}

fn track_is_active_for_mode(track_id: TrackId, mode: BounceMode) -> bool {
    match mode {
        BounceMode::Master => true,
        BounceMode::Track(tid) => tid == track_id,
        BounceMode::Clip { track_id: tid, .. } => tid == track_id,
    }
}

fn clip_included_for_mode(clip_id: ClipId, mode: BounceMode, is_note_clip: bool) -> bool {
    match mode {
        BounceMode::Master | BounceMode::Track(_) => true,
        BounceMode::Clip {
            clip_id: target,
            is_note_clip: target_is_note,
            ..
        } => target == clip_id && target_is_note == is_note_clip,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::constants::{DEFAULT_TRACK_GAIN, DEFAULT_TRACK_PAN};
    use vibez_core::effect::{EffectInfo, EffectType, ParamDescriptor, PluginDeviceInfo};
    use vibez_core::id::EffectId;
    use vibez_core::midi::{InstrumentKind, MidiNote, TrackKind};

    struct ConstantPluginInstrument {
        active: bool,
    }

    impl vibez_instruments::Instrument for ConstantPluginInstrument {
        fn instrument_kind(&self) -> InstrumentKind {
            InstrumentKind::SubtractiveSynth
        }
        fn param_descriptors(&self) -> &'static [ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, _index: usize, _value: f32) -> bool {
            false
        }
        fn get_param(&self, _index: usize) -> f32 {
            0.0
        }
        fn note_on(&mut self, _pitch: u8, _velocity: u8) {
            self.active = true;
        }
        fn note_off(&mut self, _pitch: u8) {
            self.active = false;
        }
        fn render(&mut self, buffer: &mut [f32], _channels: usize) {
            if self.active {
                buffer.iter_mut().for_each(|sample| *sample = 0.5);
            }
        }
        fn reset(&mut self) {
            self.active = false;
        }
    }

    struct ScalePluginEffect(f32);

    impl vibez_dsp::effect::AudioEffect for ScalePluginEffect {
        fn effect_type(&self) -> EffectType {
            EffectType::Gain
        }
        fn param_descriptors(&self) -> &'static [ParamDescriptor] {
            &[]
        }
        fn set_param(&mut self, _index: usize, _value: f32) -> bool {
            false
        }
        fn get_param(&self, _index: usize) -> f32 {
            self.0
        }
        fn process(&mut self, buffer: &mut [f32], _channels: usize) {
            buffer.iter_mut().for_each(|sample| *sample *= self.0);
        }
        fn reset(&mut self) {}
    }

    fn plugin_device(name: &str) -> PluginDeviceInfo {
        PluginDeviceInfo {
            format: "clap".into(),
            uid: format!("test.{name}"),
            path: format!("/test/{name}.clap").into(),
            name: name.into(),
            state_b64: Some("c3RhdGU=".into()),
        }
    }

    fn audio_of(frames: usize, value: f32) -> Arc<DecodedAudio> {
        Arc::new(DecodedAudio {
            channels: vec![vec![value; frames], vec![value; frames]],
            sample_rate: 44_100,
        })
    }

    fn bare_track(name: &str) -> TrackInfo {
        TrackInfo {
            id: TrackId::new(),
            name: name.into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::Audio,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
        }
    }

    #[test]
    fn offline_project_render_has_no_audition_bus_input() {
        let request = BounceRequest {
            tracks: Vec::new(),
            master: None,
            buses: Vec::new(),
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 512),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };

        let result = render_offline(&request);

        assert!(result
            .audio
            .channels
            .iter()
            .flatten()
            .all(|sample| sample.abs() < f32::EPSILON));
    }

    #[test]
    fn master_bounce_routes_sends_through_buses() {
        let mut track = bare_track("audio");
        track.pan = DEFAULT_TRACK_PAN;
        let bus = bare_track("A Return");
        let bus_id = bus.id;
        track.sends.push((bus_id, 1.0));
        let tid = track.id;
        let audio = audio_of(200, 0.5);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 200,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let mut req = BounceRequest {
            master: None,
            buses: vec![bus],
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 200),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let result = render_offline(&req);
        // Dry + unity send through a flat centered bus doubles the
        // contribution, exactly like the live engine.
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * 2.0;
        assert!(
            (result.audio.channels[0][10] - expected).abs() < 1e-3,
            "expected {expected}, got {}",
            result.audio.channels[0][10]
        );

        req.buses[0].solo = true;
        req.buses[0].gain = 0.5;
        let soloed = render_offline(&req);
        let wet_only = 0.5 * std::f32::consts::FRAC_1_SQRT_2 * 0.5;
        assert!(
            (soloed.audio.channels[0][10] - wet_only).abs() < 1e-3,
            "soloed return should suppress dry audio: expected {wet_only}, got {}",
            soloed.audio.channels[0][10]
        );
    }

    #[test]
    fn master_renders_single_clip() {
        let mut track = bare_track("audio");
        track.pan = DEFAULT_TRACK_PAN;
        let tid = track.id;
        let audio = audio_of(200, 0.5);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 200,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };

        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 200),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };

        let result = render_offline(&req);
        assert_eq!(result.audio.num_frames(), 200);
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        for frame in 0..200 {
            assert!((result.audio.channels[0][frame] - expected).abs() < 1e-4);
            assert!((result.audio.channels[1][frame] - expected).abs() < 1e-4);
        }
    }

    #[test]
    fn mute_silences_master_bounce() {
        let mut track = bare_track("audio");
        track.mute = true;
        let tid = track.id;
        let audio = audio_of(100, 0.7);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let out = render_offline(&req);
        assert!(out.audio.channels[0].iter().all(|&s| s.abs() < 1e-6));
    }

    #[test]
    fn track_mode_ignores_mute() {
        let mut track = bare_track("audio");
        track.mute = true;
        let tid = track.id;
        let audio = audio_of(100, 0.5);
        let cid = ClipId::new();
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);
        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let out = render_offline(&req);
        assert!(out.audio.channels[0].iter().any(|&s| s.abs() > 1e-4));
    }

    #[test]
    fn clip_mode_isolates_single_clip() {
        let mut track = bare_track("audio");
        let tid = track.id;
        track.pan = DEFAULT_TRACK_PAN;
        let cid_a = ClipId::new();
        let cid_b = ClipId::new();
        let audio_a = audio_of(100, 0.3);
        let audio_b = audio_of(100, 0.9);
        let clip_a = ClipInfo {
            id: cid_a,
            track_id: tid,
            name: "a".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let clip_b = ClipInfo {
            id: cid_b,
            track_id: tid,
            name: "b".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid_a, audio_a);
        clip_audio.insert(cid_b, audio_b);

        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: vec![clip_a, clip_b],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Clip {
                track_id: tid,
                clip_id: cid_a,
                is_note_clip: false,
            },
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let out = render_offline(&req);
        let expected = 0.3 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((out.audio.channels[0][0] - expected).abs() < 1e-4);
    }

    #[test]
    fn synth_note_clip_produces_audio() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let track = TrackInfo {
            id: tid,
            name: "Synth".into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::Instrument(InstrumentKind::SubtractiveSynth),
            color_index: 0,
            instrument: Some(InstrumentKind::SubtractiveSynth),
            native_instrument: Some(InstrumentStateInfo::SubtractiveSynth { params: Vec::new() }),
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
        };
        let note_clip = NoteClipInfo {
            id: cid,
            track_id: tid,
            name: "p".into(),
            position_beats: 0.0,
            duration_beats: 1.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
            notes: vec![MidiNote {
                pitch: 60,
                velocity: 100,
                start_beat: 0.25,
                duration_beats: 0.5,
            }],
        };
        let mut req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: vec![note_clip],
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 22_050),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let straight = render_offline(&req);
        assert!(straight.audio.channels[0].iter().any(|&s| s.abs() > 1e-3));
        req.swing = vibez_core::perform::SwingAmount::new(0.75);
        let swung = render_offline(&req);
        assert_ne!(straight.audio.channels, swung.audio.channels);
    }

    #[test]
    fn warns_on_midi_track_with_no_native_instrument() {
        let tid = TrackId::new();
        let track = TrackInfo {
            id: tid,
            name: "Plugin Stub".into(),
            gain: DEFAULT_TRACK_GAIN,
            pan: DEFAULT_TRACK_PAN,
            mute: false,
            solo: false,
            swing_offset: None,
            effects: Vec::new(),
            kind: TrackKind::Midi,
            color_index: 0,
            instrument: None,
            native_instrument: None,
            plugin_instrument: None,
            automation: Vec::new(),
            sends: Vec::new(),
        };
        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 4_410),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let out = render_offline(&req);
        assert!(!out.warnings.is_empty());
    }

    #[test]
    fn strict_render_uses_prepared_plugin_instrument_and_reports_progress() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let mut track = bare_track("Plugin Bass");
        track.id = tid;
        track.kind = TrackKind::Midi;
        track.plugin_instrument = Some(plugin_device("Surge XT"));
        let note_clip = NoteClipInfo {
            id: cid,
            track_id: tid,
            name: "bass".into(),
            position_beats: 0.0,
            duration_beats: 1.0,
            loop_enabled: false,
            loop_start_beats: 0.0,
            loop_end_beats: 0.0,
            groove_grid: vibez_core::perform::GrooveGrid::Sixteenth,
            notes: vec![MidiNote {
                pitch: 36,
                velocity: 100,
                start_beat: 0.0,
                duration_beats: 1.0,
            }],
        };
        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: vec![note_clip],
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 2_048),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let mut plugins = OfflinePlugins::default();
        plugins
            .instruments
            .insert(tid, Box::new(ConstantPluginInstrument { active: false }));
        let mut progress = Vec::new();

        let result =
            render_offline_with_plugins(&req, &mut plugins, |value| progress.push(value)).unwrap();

        assert!(result.audio.channels[0].iter().any(|sample| *sample > 0.1));
        assert!(
            plugins.instruments.contains_key(&tid),
            "renderer must return the plugin for main-thread teardown"
        );
        assert_eq!(progress.first(), Some(&0));
        assert_eq!(progress.last(), Some(&100));
        assert!(progress.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn strict_render_fails_when_declared_plugin_was_not_prepared() {
        let mut track = bare_track("Plugin Bass");
        track.kind = TrackKind::Midi;
        track.plugin_instrument = Some(plugin_device("Surge XT"));
        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: Vec::new(),
            note_clips: Vec::new(),
            clip_audio: HashMap::new(),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 512),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };

        let mut plugins = OfflinePlugins::default();
        let error = match render_offline_with_plugins(&req, &mut plugins, |_| {}) {
            Ok(_) => panic!("missing plugin must fail the strict render"),
            Err(error) => error,
        };

        assert!(error.contains("Surge XT"));
        assert!(error.contains("not prepared"));
    }

    #[test]
    fn strict_render_uses_plugin_effects_on_tracks_buses_and_master() {
        fn plugin_effect() -> EffectInfo {
            EffectInfo {
                id: EffectId::new(),
                effect_type: EffectType::Gain,
                bypass: false,
                params: Vec::new(),
                plugin: Some(plugin_device("Scale")),
            }
        }

        let mut track = bare_track("audio");
        let tid = track.id;
        let cid = ClipId::new();
        let track_fx = plugin_effect();
        track.effects.push(track_fx.clone());
        let mut bus = bare_track("Return");
        let bus_fx = plugin_effect();
        bus.effects.push(bus_fx.clone());
        track.sends.push((bus.id, 1.0));
        let mut master = bare_track("Master");
        master.id = TrackId::MASTER;
        let master_fx = plugin_effect();
        master.effects.push(master_fx.clone());
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "audio".into(),
            position: 0,
            source_offset: 0,
            duration: 64,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let req = BounceRequest {
            master: Some(master),
            buses: vec![bus],
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio: HashMap::from([(cid, audio_of(64, 1.0))]),
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Master,
            range_samples: (0, 64),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let mut plugins = OfflinePlugins::default();
        plugins
            .effects
            .insert(track_fx.id, Box::new(ScalePluginEffect(0.5)));
        plugins
            .effects
            .insert(bus_fx.id, Box::new(ScalePluginEffect(0.5)));
        plugins
            .effects
            .insert(master_fx.id, Box::new(ScalePluginEffect(0.5)));

        let result = render_offline_with_plugins(&req, &mut plugins, |_| {}).unwrap();

        // Track: 1 * .5. Dry + return(.5) = .75 before centered pan,
        // then master .5.
        let expected = 0.75 * std::f32::consts::FRAC_1_SQRT_2 * 0.5;
        assert!((result.audio.channels[0][10] - expected).abs() < 1e-3);
        assert_eq!(
            plugins.effects.len(),
            3,
            "every effect must be returned for main-thread teardown"
        );
    }

    #[test]
    fn effect_chain_applied_during_bounce() {
        let tid = TrackId::new();
        let cid = ClipId::new();
        let mut track = bare_track("audio");
        track.id = tid;
        // Gain of 0.5 halves the bounce output
        track.effects.push(EffectInfo {
            id: EffectId::new(),
            effect_type: EffectType::Gain,
            bypass: false,
            params: vec![0.5],
            plugin: None,
        });
        let audio = audio_of(100, 1.0);
        let clip = ClipInfo {
            id: cid,
            track_id: tid,
            name: "c".into(),
            position: 0,
            source_offset: 0,
            duration: 100,
            source: None,
            file_path: None,
            loop_enabled: false,
            loop_start: 0,
            loop_end: 0,
            original_bpm: None,
            warped: false,
            warped_to_bpm: None,
        };
        let mut clip_audio = HashMap::new();
        clip_audio.insert(cid, audio);

        let req = BounceRequest {
            master: None,
            buses: Vec::new(),
            tracks: vec![track],
            audio_clips: vec![clip],
            note_clips: Vec::new(),
            clip_audio,
            sampler_audio: HashMap::new(),
            drum_pad_audio: HashMap::new(),
            mode: BounceMode::Track(tid),
            range_samples: (0, 100),
            bpm: 120.0,
            sample_rate: 44_100,
            swing: vibez_core::perform::SwingAmount::STRAIGHT,
        };
        let out = render_offline(&req);
        let peak = out.audio.channels[0]
            .iter()
            .map(|s| s.abs())
            .fold(0.0_f32, f32::max);
        let expected = 0.5 * std::f32::consts::FRAC_1_SQRT_2;
        assert!((peak - expected).abs() < 1e-3, "peak {peak}");
    }
}
