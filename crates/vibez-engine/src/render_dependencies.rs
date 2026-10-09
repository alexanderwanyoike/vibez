use super::*;
use vibez_core::routing::{ExternalInputDescriptor, RoutingChannel, RoutingEffect, SourceTap};

pub(super) fn selected_track(mode: BounceMode) -> Option<TrackId> {
    match mode {
        BounceMode::Master => None,
        BounceMode::Track(track)
        | BounceMode::Clip {
            track_id: track, ..
        } => Some(track),
    }
}

pub(super) fn all_channels(req: &BounceRequest) -> impl Iterator<Item = &TrackInfo> {
    req.tracks
        .iter()
        .chain(req.buses.iter())
        .chain(req.master.iter())
}

pub(super) fn potential_sends(track: &TrackInfo) -> Vec<(TrackId, f32)> {
    let mut sends = track.sends.clone();
    vibez_core::routing::reserve_automated_sends(&mut sends, &track.automation);
    sends
}

pub fn potential_dependency_ids(req: &BounceRequest) -> HashSet<TrackId> {
    dependency_taps(req, None).into_keys().collect()
}

fn rank(tap: SourceTap) -> u8 {
    match tap {
        SourceTap::BeforeEffects => 0,
        SourceTap::AfterEffects => 1,
        SourceTap::AfterFader => 2,
    }
}
fn require(scope: &mut HashMap<TrackId, SourceTap>, channel: TrackId, tap: SourceTap) {
    scope
        .entry(channel)
        .and_modify(|existing| {
            if rank(tap) > rank(*existing) {
                *existing = tap;
            }
        })
        .or_insert(tap);
}

pub(super) struct RenderDependencies {
    pub taps: HashMap<TrackId, SourceTap>,
    pub graph_channels: Vec<RoutingChannel>,
}

pub(super) fn actual_dependencies(
    req: &BounceRequest,
    plugins: Option<&OfflinePlugins>,
) -> RenderDependencies {
    let mut metadata: HashMap<EffectId, Vec<ExternalInputDescriptor>> = HashMap::new();
    for info in all_channels(req).flat_map(|channel| &channel.effects) {
        let inputs = if info.plugin.is_some() {
            plugins
                .and_then(|plugins| plugins.effects.get(&info.id))
                .map_or_else(Vec::new, |effect| effect.external_inputs().to_vec())
        } else {
            vibez_dsp::factory::create_effect(info.effect_type, req.sample_rate as f32)
                .external_inputs()
                .to_vec()
        };
        metadata.insert(info.id, inputs);
    }
    let taps = dependency_taps(req, Some(&metadata));
    let mut graph_channels: Vec<_> = all_channels(req)
        .filter(|channel| taps.contains_key(&channel.id))
        .map(|channel| RoutingChannel {
            id: channel.id,
            is_bus: req.buses.iter().any(|bus| bus.id == channel.id) || channel.id.is_master(),
            sends: potential_sends(channel),
            effects: if taps[&channel.id] == SourceTap::BeforeEffects {
                vec![]
            } else {
                channel
                    .effects
                    .iter()
                    .map(|effect| RoutingEffect {
                        id: effect.id,
                        inputs: metadata[&effect.id].clone(),
                        assignments: effect.sidechains.clone(),
                        inactive_inputs: effect.inactive_sidechains.clone(),
                    })
                    .collect()
            },
        })
        .collect();
    if !graph_channels.iter().any(|channel| channel.id.is_master()) {
        graph_channels.push(RoutingChannel {
            id: TrackId::MASTER,
            is_bus: true,
            sends: vec![],
            effects: vec![],
        });
    }
    RenderDependencies {
        taps,
        graph_channels,
    }
}

pub(super) fn validate_plugins(
    req: &BounceRequest,
    plugins: &OfflinePlugins,
    dependencies: &RenderDependencies,
) -> Result<(), String> {
    for channel in all_channels(req).filter(|channel| dependencies.taps.contains_key(&channel.id)) {
        if let Some(device) = &channel.plugin_instrument {
            if !plugins.instruments.contains_key(&channel.id) {
                return Err(format!(
                    "Track '{}' requires {} plugin instrument '{}': {}",
                    channel.name,
                    device.format.to_uppercase(),
                    device.name,
                    plugins
                        .instrument_failures
                        .get(&channel.id)
                        .map_or("not prepared", String::as_str)
                ));
            }
        }
        if dependencies.taps[&channel.id] == SourceTap::BeforeEffects {
            continue;
        }
        for effect in &channel.effects {
            if let Some(device) = &effect.plugin {
                if !plugins.effects.contains_key(&effect.id) {
                    return Err(format!(
                        "Channel '{}' requires {} effect '{}': {}",
                        channel.name,
                        device.format.to_uppercase(),
                        device.name,
                        plugins
                            .failures
                            .get(&effect.id)
                            .map_or("not prepared", String::as_str)
                    ));
                }
            }
        }
    }
    Ok(())
}

fn dependency_taps(
    req: &BounceRequest,
    metadata: Option<&HashMap<EffectId, Vec<ExternalInputDescriptor>>>,
) -> HashMap<TrackId, SourceTap> {
    let mut taps = HashMap::new();
    if let Some(track) = selected_track(req.mode) {
        require(&mut taps, track, SourceTap::AfterFader);
    } else {
        for channel in all_channels(req) {
            require(&mut taps, channel.id, SourceTap::AfterFader);
        }
    }
    loop {
        let before = taps.clone();
        for channel in all_channels(req) {
            let Some(tap) = taps.get(&channel.id).copied() else {
                continue;
            };
            if tap != SourceTap::BeforeEffects {
                for effect in &channel.effects {
                    for route in &effect.sidechains {
                        if !effect.inactive_sidechains.contains(&route.input_id)
                            && metadata.is_none_or(|metadata| {
                                metadata[&effect.id]
                                    .iter()
                                    .any(|input| route.matches(input))
                            })
                            && !route.source.is_master()
                            && all_channels(req).any(|source| source.id == route.source)
                        {
                            require(
                                &mut taps,
                                route.source,
                                if metadata.is_some() {
                                    route.tap
                                } else {
                                    SourceTap::AfterFader
                                },
                            );
                        }
                    }
                }
            }
            if req.buses.iter().any(|bus| bus.id == channel.id) {
                for source in all_channels(req).filter(|source| {
                    potential_sends(source).iter().any(|(bus, amount)| {
                        *bus == channel.id && *amount > vibez_core::routing::SEND_SILENCE_THRESHOLD
                    })
                }) {
                    require(&mut taps, source.id, SourceTap::AfterFader);
                }
            }
        }
        if taps == before {
            break;
        }
    }
    taps
}
