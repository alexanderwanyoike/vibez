use super::*;

pub(super) fn render_offline_inner(
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
                Some(audio) => {
                    engine.playback_source.clips.push(EngineClip {
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
                }
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
                nc.resolved_start_marker_beats(),
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
            if track.instrument.is_some() {
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
                );
            } else {
                // Offline bounce walks the requested range without arrangement looping.
                track.render(pos, block, CHANNELS, None);
            }
            // Keep insert processing consistent with the live path in engine_render.rs.
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
                if *amount <= vibez_core::routing::SEND_SILENCE_THRESHOLD {
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
