//! Whether two Project Track states hold the same channels and devices, so undo can move between them with control commands.

use super::ui_types::same_audio;
use super::{ProjectTrack, ProjectTracksState, UiDrumPad, UiEffect};

impl ProjectTracksState {
    /// Mixer values, send levels and built-in device values may differ, since
    /// undo sends them as control commands. Meter readings and reported
    /// latencies change while the song plays, descriptors and GUI flags
    /// follow from the device itself, and names and colours are only drawn.
    pub fn same_channel_layout(&self, other: &Self) -> bool {
        let Self {
            reduced_latency_monitoring,
            tracks,
            master,
            buses,
            next_track_number,
        } = self;
        *reduced_latency_monitoring == other.reduced_latency_monitoring
            && *next_track_number == other.next_track_number
            && same_track(master, &other.master)
            && all_same(tracks, &other.tracks, same_track)
            && all_same(buses, &other.buses, same_track)
    }
}

fn all_same<T>(left: &[T], right: &[T], same: impl Fn(&T, &T) -> bool) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(l, r)| same(l, r))
}

fn same_track(track: &ProjectTrack, other: &ProjectTrack) -> bool {
    let ProjectTrack {
        id,
        name: _,
        gain: _,
        pan: _,
        mute: _,
        solo: _,
        audio_input_route,
        input_monitoring,
        swing_offset: _,
        peak_l: _,
        peak_r: _,
        effects,
        sends,
        kind,
        color_index: _,
        has_instrument,
        instrument_kind,
        sample_name,
        sample_source,
        sample_audio,
        instrument_params,
        drum_rack_pads,
        selected_drum_pad: _,
        plugin_instrument_name,
        plugin_instrument_ref,
        plugin_instrument_descriptors: _,
        has_plugin_instrument_gui: _,
        instrument_latency_samples: _,
    } = track;
    *id == other.id
        && *audio_input_route == other.audio_input_route
        && *input_monitoring == other.input_monitoring
        && all_same(effects, &other.effects, same_effect)
        && sends.len() == other.sends.len()
        && sends
            .iter()
            .zip(&other.sends)
            .all(|((bus, _), (other_bus, _))| bus == other_bus)
        && *kind == other.kind
        && *has_instrument == other.has_instrument
        && *instrument_kind == other.instrument_kind
        && *sample_name == other.sample_name
        && *sample_source == other.sample_source
        && same_audio(sample_audio, &other.sample_audio)
        && (instrument_kind.is_some() || *instrument_params == other.instrument_params)
        && all_same(drum_rack_pads, &other.drum_rack_pads, same_pad)
        && *plugin_instrument_name == other.plugin_instrument_name
        && *plugin_instrument_ref == other.plugin_instrument_ref
}

fn same_effect(effect: &UiEffect, other: &UiEffect) -> bool {
    let UiEffect {
        reconfiguration_failed,
        latency_samples: _,
        id,
        effect_type,
        bypass: _,
        params,
        descriptors: _,
        plugin_name,
        has_plugin_gui: _,
        plugin_ref,
        sidechains,
        inactive_sidechains,
        external_inputs,
    } = effect;
    *reconfiguration_failed == other.reconfiguration_failed
        && *id == other.id
        && *effect_type == other.effect_type
        && (plugin_ref.is_none() || *params == other.params)
        && *plugin_name == other.plugin_name
        && *plugin_ref == other.plugin_ref
        && *sidechains == other.sidechains
        && *inactive_sidechains == other.inactive_sidechains
        && *external_inputs == other.external_inputs
}

fn same_pad(pad: &UiDrumPad, other: &UiDrumPad) -> bool {
    let UiDrumPad {
        name,
        source,
        audio,
        choke_group,
        params,
    } = pad;
    *name == other.name
        && *source == other.source
        && same_audio(audio, &other.audio)
        && *choke_group == other.choke_group
        && params.len() == other.params.len()
}
