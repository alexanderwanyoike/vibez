use super::*;

pub(super) fn validate_offline_plugins(
    req: &BounceRequest,
    plugins: &OfflinePlugins,
) -> Result<(), String> {
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

pub(super) fn return_offline_plugins(
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

pub(super) fn return_plugin_effects(
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

pub(super) fn track_is_active_for_mode(track_id: TrackId, mode: BounceMode) -> bool {
    match mode {
        BounceMode::Master => true,
        BounceMode::Track(tid) => tid == track_id,
        BounceMode::Clip { track_id: tid, .. } => tid == track_id,
    }
}

pub(super) fn clip_included_for_mode(
    clip_id: ClipId,
    mode: BounceMode,
    is_note_clip: bool,
) -> bool {
    match mode {
        BounceMode::Master | BounceMode::Track(_) => true,
        BounceMode::Clip {
            clip_id: target,
            is_note_clip: target_is_note,
            ..
        } => target == clip_id && target_is_note == is_note_clip,
    }
}
