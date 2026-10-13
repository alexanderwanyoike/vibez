//! Solo scopes follow audible paths without promoting control-only contributors.

use super::*;
use vibez_core::routing::{EdgeKind, NodeStage};

impl PreparedRouting {
    pub(crate) fn select_audible_returns(
        &mut self,
        tracks: &[crate::mixer::EngineTrack],
        buses: &[crate::mixer::EngineTrack],
        master: &crate::mixer::EngineTrack,
        track_solo: bool,
        bus_solo: bool,
    ) {
        for channel in &mut self.channels {
            channel.audible_return = channel.id.is_master()
                || (matches!(channel.binding, ChannelIndex::Bus(_))
                    && !bus_solo
                    && !self.detector_buses.contains(&channel.id))
                || channel
                    .binding
                    .get(tracks, buses, master)
                    .is_some_and(|track| (!track_solo && !bus_solo) || track.solo);
        }
        if !track_solo || bus_solo {
            return;
        }
        // Graph order lets an audible send propagate through a Bus chain.
        // Control-input edges cannot grant that Bus an audible return.
        for &index in &self.graph.order {
            if self.graph.nodes[index].stage != NodeStage::Sum
                || self.graph.nodes[index].channel.is_master()
            {
                continue;
            }
            let audible = self.nodes[index].incoming.iter().any(|&edge_index| {
                let edge = self.graph.edges[edge_index];
                edge.kind == EdgeKind::Send
                    && self.channels[self.nodes[edge.from].channel].audible_return
            });
            self.channels[self.nodes[index].channel].audible_return |= audible;
        }
    }
}
