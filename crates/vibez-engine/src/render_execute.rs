use super::*;
use crate::engine::{AudioEngine, OfflineRoutingSetup};

pub(super) fn render_offline_inner(
    req: &BounceRequest,
    mut plugins: Option<&mut OfflinePlugins>,
    mut progress: impl FnMut(u8),
) -> Result<BounceResult, String> {
    if req.sample_rate == 0
        || !req.bpm.is_finite()
        || req.bpm <= 0.0
        || req.range_samples.1 < req.range_samples.0
    {
        return Err("Invalid offline sample rate, tempo or render range".into());
    }
    let (start, end) = req.range_samples;
    let frames =
        usize::try_from(end - start).map_err(|_| "Offline range exceeds addressable storage")?;
    progress(0);
    let dependencies = dependencies::actual_dependencies(req, plugins.as_deref());
    if let Some(plugins) = plugins.as_deref() {
        dependencies::validate_plugins(req, plugins, &dependencies)?;
    }
    let mut reports = Vec::new();
    let mut controls = Vec::new();
    for channel in &dependencies.graph_channels {
        let info = dependencies::all_channels(req).find(|info| info.id == channel.id);
        let instrument_latency = plugins
            .as_deref()
            .and_then(|plugins| plugins.instruments.get(&channel.id))
            .map_or(0, |instrument| instrument.latency_samples());
        reports.push((
            vibez_core::routing::RoutingNode {
                channel: channel.id,
                stage: vibez_core::routing::NodeStage::Source,
            },
            instrument_latency,
        ));
        if let Some(info) = info {
            controls.extend(info.automation.iter().map(|lane| (channel.id, lane.target)));
            for effect in &channel.effects {
                let latency = plugins
                    .as_deref()
                    .and_then(|plugins| plugins.effects.get(&effect.id))
                    .map(|effect| effect.latency_samples())
                    .unwrap_or_else(|| {
                        info.effects
                            .iter()
                            .find(|info| info.id == effect.id)
                            .map_or(0, |info| {
                                vibez_dsp::factory::create_effect_with_params(
                                    info.effect_type,
                                    req.sample_rate as f32,
                                    &info.params,
                                )
                                .latency_samples()
                            })
                    });
                reports.push((
                    vibez_core::routing::RoutingNode {
                        channel: channel.id,
                        stage: vibez_core::routing::NodeStage::Effect(effect.id),
                    },
                    latency,
                ));
            }
        }
    }
    let mut routing = crate::routing::PreparedRouting::prepare_compensated(
        &dependencies.graph_channels,
        BLOCK_FRAMES,
        &reports,
        &[],
        1,
    )?;
    routing.configure_automation(&controls)?;
    let timing = crate::bounce_timing::BounceTiming::prepare(
        (start, end),
        routing.compensation.output_latency,
    )
    .map_err(str::to_owned)?;
    let mut warnings = Vec::new();
    let mut tracks = Vec::new();
    let mut buses = Vec::new();
    for info in req
        .tracks
        .iter()
        .filter(|info| dependencies.taps.contains_key(&info.id))
    {
        tracks.push(prepare::channel(
            req,
            info,
            dependencies.taps[&info.id],
            plugins.as_deref_mut(),
            &mut warnings,
        ));
    }
    for info in req
        .buses
        .iter()
        .filter(|info| dependencies.taps.contains_key(&info.id))
    {
        buses.push(prepare::channel(
            req,
            info,
            dependencies.taps[&info.id],
            plugins.as_deref_mut(),
            &mut warnings,
        ));
    }
    let master = req
        .master
        .as_ref()
        .filter(|info| dependencies.taps.contains_key(&info.id))
        .map(|info| {
            prepare::channel(
                req,
                info,
                dependencies.taps[&info.id],
                plugins.as_deref_mut(),
                &mut warnings,
            )
        })
        .unwrap_or_else(|| EngineTrack::new(TrackId::MASTER));
    let mut left = Vec::with_capacity(frames);
    let mut right = Vec::with_capacity(frames);
    let mut scratch = vec![0.0; BLOCK_FRAMES * CHANNELS];
    let mut selected = vec![0.0; BLOCK_FRAMES * CHANNELS];
    let selected_track = dependencies::selected_track(req.mode);
    let mut engine = AudioEngine::for_offline_routing(OfflineRoutingSetup {
        sample_rate: req.sample_rate,
        bpm: req.bpm,
        swing: req.swing,
        tracks,
        buses,
        master,
        routing,
    });
    let mut position = timing.render_start;
    let mut failure = None;
    while position < timing.render_end {
        let block = (timing.render_end - position).min(BLOCK_FRAMES as u64) as usize;
        let output = &mut scratch[..block * CHANNELS];
        let selected_output = &mut selected[..block * CHANNELS];
        engine.render_offline_routing_segment(
            position,
            output,
            selected_track.map(|id| (id, selected_output)),
        );
        if let Some(reason) = engine.offline_compensation_failure() {
            failure = Some(reason);
            break;
        }
        if let Some(selection) = timing.block_selection(position, block) {
            let written = if selected_track.is_some() {
                &selected[..block * CHANNELS]
            } else {
                output
            };
            for frame in
                written[selection.start * CHANNELS..selection.end * CHANNELS].chunks_exact(CHANNELS)
            {
                left.push(frame[0]);
                right.push(frame[1]);
            }
        }
        position += block as u64;
        progress(if timing.render_end == 0 {
            100
        } else {
            ((position as u128 * 100) / timing.render_end as u128).min(100) as u8
        });
    }
    if let Some(plugins) = plugins {
        let (mut tracks, mut buses, mut master) = engine.take_offline_channels();
        prepare::return_plugins(req, plugins, &mut tracks, &mut buses, &mut master);
    }
    if let Some(reason) = failure {
        return Err(reason);
    }
    progress(100);
    Ok(BounceResult {
        audio: DecodedAudio {
            channels: vec![left, right],
            sample_rate: req.sample_rate,
        },
        warnings,
    })
}
