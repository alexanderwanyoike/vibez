//! Resolve drum pad display names and scalar settings before audio-thread transfer.

use vibez_core::track::DrumPadState;

#[derive(Debug, PartialEq)]
pub struct PreparedDrumPadState {
    pub name: Option<String>,
    pub gain: f32,
    pub pan: f32,
    pub start: f32,
    pub end: f32,
    pub fade_in_ms: f32,
    pub fade_out_ms: f32,
    pub coarse_tune: i8,
    pub fine_tune: f32,
    pub one_shot: bool,
    pub choke_group: Option<u8>,
}

impl From<DrumPadState> for PreparedDrumPadState {
    fn from(state: DrumPadState) -> Self {
        // Source paths and provenance are unused by DSP and must drop on this sender.
        let name = state.name.or_else(|| state.source.as_ref().map(|source| source.display_name()));
        Self {
            name,
            gain: state.gain,
            pan: state.pan,
            start: state.start,
            end: state.end,
            fade_in_ms: state.fade_in_ms,
            fade_out_ms: state.fade_out_ms,
            coarse_tune: state.coarse_tune,
            fine_tune: state.fine_tune,
            one_shot: state.one_shot,
            choke_group: state.choke_group,
        }
    }
}
