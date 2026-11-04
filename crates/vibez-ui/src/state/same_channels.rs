//! Whether two Project Track states describe the same channels, apart from what the running engine reports back.

use super::ui_types::same_audio;
use super::{ProjectTrack, ProjectTracksState, UiDrumPad, UiEffect};

impl ProjectTracksState {
    /// Meter readings and reported latencies change while the song plays;
    /// descriptors and GUI flags follow from the device itself.
    pub fn same_channels(&self, other: &Self) -> bool {
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
        name,
        gain,
        pan,
        mute,
        solo,
        audio_input_route,
        input_monitoring,
        swing_offset,
        peak_l: _,
        peak_r: _,
        effects,
        sends,
        kind,
        color_index,
        has_instrument,
        instrument_kind,
        sample_name,
        sample_source,
        sample_audio,
        instrument_params,
        drum_rack_pads,
        selected_drum_pad,
        plugin_instrument_name,
        plugin_instrument_ref,
        plugin_instrument_descriptors: _,
        has_plugin_instrument_gui: _,
        instrument_latency_samples: _,
    } = track;
    *id == other.id
        && *name == other.name
        && *gain == other.gain
        && *pan == other.pan
        && *mute == other.mute
        && *solo == other.solo
        && *audio_input_route == other.audio_input_route
        && *input_monitoring == other.input_monitoring
        && *swing_offset == other.swing_offset
        && all_same(effects, &other.effects, same_effect)
        && *sends == other.sends
        && *kind == other.kind
        && *color_index == other.color_index
        && *has_instrument == other.has_instrument
        && *instrument_kind == other.instrument_kind
        && *sample_name == other.sample_name
        && *sample_source == other.sample_source
        && same_audio(sample_audio, &other.sample_audio)
        && *instrument_params == other.instrument_params
        && all_same(drum_rack_pads, &other.drum_rack_pads, same_pad)
        && *selected_drum_pad == other.selected_drum_pad
        && *plugin_instrument_name == other.plugin_instrument_name
        && *plugin_instrument_ref == other.plugin_instrument_ref
}

fn same_effect(effect: &UiEffect, other: &UiEffect) -> bool {
    let UiEffect {
        reconfiguration_failed,
        latency_samples: _,
        id,
        effect_type,
        bypass,
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
        && *bypass == other.bypass
        && *params == other.params
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
        && *params == other.params
}
