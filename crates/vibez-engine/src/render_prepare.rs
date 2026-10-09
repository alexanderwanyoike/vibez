use super::*;
use crate::mixer::{EngineClip, EngineNoteClip};
use vibez_core::effect::EffectInfo;
use vibez_core::routing::SourceTap;

pub(super) fn channel(
    req: &BounceRequest,
    info: &TrackInfo,
    tap: SourceTap,
    mut plugins: Option<&mut OfflinePlugins>,
    warnings: &mut Vec<String>,
) -> EngineTrack {
    let mut channel = EngineTrack::new(info.id);
    channel.gain = info.gain;
    channel.pan = info.pan;
    channel.swing_offset = info.swing_offset;
    channel.sends = info.sends.clone();
    let selected = dependencies::selected_track(req.mode);
    channel.set_manual_mute(
        if selected == Some(info.id) {
            false
        } else {
            info.mute
        },
        true,
    );
    channel.solo = matches!(req.mode, BounceMode::Master) && info.solo;
    channel.playback_source.automation = info
        .automation
        .iter()
        .filter(|lane| {
            !(selected == Some(info.id)
                && lane.target == vibez_core::automation::AutomationTarget::TrackMute)
        })
        .cloned()
        .collect();
    if info.plugin_instrument.is_some() {
        channel.instrument = plugins
            .as_deref_mut()
            .and_then(|plugins| plugins.instruments.remove(&info.id));
        if channel.instrument.is_none() {
            warnings.push(format!(
                "Track '{}' plugin instrument is unavailable in this render",
                info.name
            ));
        }
    } else if let Some(kind) = info.instrument {
        let mut instrument = vibez_instruments::create_instrument(kind, req.sample_rate as f32);
        if let Some(state) = &info.native_instrument {
            match state {
                InstrumentStateInfo::SubtractiveSynth { params } => {
                    for (index, value) in params.iter().enumerate() {
                        instrument.set_param(index, *value);
                    }
                }
                InstrumentStateInfo::Sampler { params, .. } => {
                    for (index, value) in params.iter().enumerate() {
                        instrument.set_param(index, *value);
                    }
                    if let Some((audio, name)) = req.sampler_audio.get(&info.id) {
                        instrument.load_sample(Arc::clone(audio), name.clone());
                    }
                }
                InstrumentStateInfo::DrumRack { pads } => {
                    for (index, pad) in pads.iter().enumerate() {
                        instrument.set_drum_pad_state(index, pad.clone());
                        if let Some((audio, name)) = req.drum_pad_audio.get(&(info.id, index)) {
                            instrument.load_drum_pad_sample(index, Arc::clone(audio), name.clone());
                        }
                    }
                }
            }
        }
        channel.instrument = Some(instrument);
    } else if info.kind.is_midi() {
        warnings.push(format!(
            "Track '{}' has no native instrument; any plugin instrument will not render",
            info.name
        ));
    }
    if tap != SourceTap::BeforeEffects {
        channel.effects = effects(
            &info.effects,
            req.sample_rate,
            plugins,
            warnings,
            &info.name,
        );
    }
    for clip in req
        .audio_clips
        .iter()
        .filter(|clip| clip.track_id == info.id)
    {
        if selected == Some(info.id) && !clip_included(clip.id, req.mode, false) {
            continue;
        }
        if let Some(audio) = req.clip_audio.get(&clip.id) {
            channel.playback_source.clips.push(EngineClip {
                id: clip.id,
                audio: Arc::clone(audio),
                position: clip.position,
                source_offset: clip.source_offset,
                start_marker: clip.resolved_start_marker(audio.num_frames() as u64),
                duration: clip.duration,
                loop_enabled: clip.loop_enabled,
                loop_start: clip.loop_start,
                loop_end: clip.loop_end,
                linear_gain: clip.gain_db.linear(),
                fades: clip.fades.clamped_to(clip.duration),
                playback_direction: clip.playback_direction,
                warp_markers: clip.warp_markers.clone(),
            });
        } else {
            warnings.push(format!("Clip '{}' audio missing, skipped", clip.name));
        }
    }
    for clip in req
        .note_clips
        .iter()
        .filter(|clip| clip.track_id == info.id)
    {
        if selected == Some(info.id) && !clip_included(clip.id, req.mode, true) {
            continue;
        }
        channel.playback_source.note_clips.push(EngineNoteClip::new(
            clip.id,
            clip.position_beats,
            clip.duration_beats,
            clip.notes.clone(),
            clip.resolved_start_marker_beats(),
            clip.loop_enabled,
            clip.loop_start_beats,
            clip.loop_end_beats,
            clip.groove_grid,
        ));
    }
    channel
}

fn effects(
    info: &[EffectInfo],
    sample_rate: u32,
    mut plugins: Option<&mut OfflinePlugins>,
    warnings: &mut Vec<String>,
    channel: &str,
) -> Vec<EffectSlot> {
    info.iter()
        .filter_map(|info| {
            let effect = if let Some(device) = &info.plugin {
                match plugins
                    .as_deref_mut()
                    .and_then(|plugins| plugins.effects.remove(&info.id))
                {
                    Some(effect) => effect,
                    None => {
                        warnings.push(format!(
                            "Channel '{channel}' plugin effect '{}' is unavailable in this render",
                            device.name
                        ));
                        return None;
                    }
                }
            } else {
                vibez_dsp::factory::create_effect_with_params(
                    info.effect_type,
                    sample_rate as f32,
                    &info.params,
                )
            };
            Some(EffectSlot {
                id: info.id,
                effect,
                bypass: info.bypass,
            })
        })
        .collect()
}

fn clip_included(id: ClipId, mode: BounceMode, note: bool) -> bool {
    match mode {
        BounceMode::Master | BounceMode::Track(_) => true,
        BounceMode::Clip {
            clip_id,
            is_note_clip,
            ..
        } => id == clip_id && note == is_note_clip,
    }
}

pub(super) fn return_plugins(
    req: &BounceRequest,
    plugins: &mut OfflinePlugins,
    tracks: &mut [EngineTrack],
    buses: &mut [EngineTrack],
    master: &mut EngineTrack,
) {
    let ids: HashSet<_> = dependencies::all_channels(req)
        .flat_map(|channel| &channel.effects)
        .filter(|effect| effect.plugin.is_some())
        .map(|effect| effect.id)
        .collect();
    for channel in tracks
        .iter_mut()
        .chain(buses.iter_mut())
        .chain(std::iter::once(master))
    {
        if dependencies::all_channels(req)
            .find(|info| info.id == channel.id)
            .is_some_and(|info| info.plugin_instrument.is_some())
        {
            if let Some(mut instrument) = channel.instrument.take() {
                instrument.finish_offline_processing();
                plugins.instruments.insert(channel.id, instrument);
            }
        }
        let mut index = 0;
        while index < channel.effects.len() {
            if ids.contains(&channel.effects[index].id) {
                let mut slot = channel.effects.remove(index);
                slot.effect.finish_offline_processing();
                plugins.effects.insert(slot.id, slot.effect);
            } else {
                index += 1;
            }
        }
    }
}
