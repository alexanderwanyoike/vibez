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
    let routing =
        crate::routing::PreparedRouting::prepare(&dependencies.graph_channels, BLOCK_FRAMES)?;
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
    let mut position = 0u64;
    while position < end {
        let block = (end - position).min(BLOCK_FRAMES as u64) as usize;
        let output = &mut scratch[..block * CHANNELS];
        let selected_output = &mut selected[..block * CHANNELS];
        engine.render_offline_routing_segment(
            position,
            output,
            selected_track.map(|id| (id, selected_output)),
        );
        if position + block as u64 > start {
            let first_frame = start.saturating_sub(position) as usize;
            let written = if selected_track.is_some() {
                &selected[..block * CHANNELS]
            } else {
                output
            };
            for frame in written[first_frame * CHANNELS..].chunks_exact(CHANNELS) {
                left.push(frame[0]);
                right.push(frame[1]);
            }
        }
        position += block as u64;
        progress(if end == 0 {
            100
        } else {
            ((position as u128 * 100) / end as u128).min(100) as u8
        });
    }
    if let Some(plugins) = plugins {
        let (mut tracks, mut buses, mut master) = engine.take_offline_channels();
        prepare::return_plugins(req, plugins, &mut tracks, &mut buses, &mut master);
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
