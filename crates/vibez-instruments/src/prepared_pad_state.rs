//! Resolve drum pad display names and Sampler controls before audio-thread transfer.

use vibez_core::track::DrumPadState;

use crate::sampler::param;

#[derive(Debug, PartialEq)]
pub struct PreparedDrumPadState {
    pub name: Option<String>,
    /// The pad's Sampler controls, a fixed array so the audio thread frees
    /// nothing when it takes them.
    pub params: [f32; param::COUNT],
    pub choke_group: Option<u8>,
}

impl From<DrumPadState> for PreparedDrumPadState {
    fn from(state: DrumPadState) -> Self {
        // Source paths and provenance are unused by DSP and must drop on this sender.
        let params = crate::drum_rack::pad_params(&state);
        let name = state
            .name
            .or_else(|| state.source.as_ref().map(|source| source.display_name()));
        Self {
            name,
            params,
            choke_group: state.choke_group,
        }
    }
}
