use super::*;

impl AudioEngine {
    // Returning the inline owner avoids allocating a command box in the callback.
    #[allow(clippy::result_large_err)]
    pub(super) fn handle_compensation_command(
        &mut self,
        cmd: EngineCommand,
    ) -> Result<(), EngineCommand> {
        match cmd {
            EngineCommand::UpdateAutomationRouting(mut prepared) => {
                let generation = prepared.compensation.generation;
                if let Some(mut previous) = self.routing.take() {
                    let compatible = previous.graph.nodes == prepared.graph.nodes
                        && previous.graph.edges == prepared.graph.edges
                        && previous.device_latencies == prepared.device_latencies
                        && previous.compensation.edge_delays == prepared.compensation.edge_delays
                        && previous.max_frames == prepared.max_frames;
                    if compatible {
                        std::mem::swap(&mut previous.compensation, &mut prepared.compensation);
                        std::mem::swap(&mut previous.bypass_delays, &mut prepared.bypass_delays);
                        std::mem::swap(&mut previous.presentation, &mut prepared.presentation);
                        std::mem::swap(
                            &mut previous.presentation_start,
                            &mut prepared.presentation_start,
                        );
                        std::mem::swap(&mut previous.channel_clocks, &mut prepared.channel_clocks);
                        for next in &mut prepared.automation_controls {
                            if let Some(old) = previous
                                .automation_controls
                                .iter_mut()
                                .find(|old| old.track == next.track && old.target == next.target)
                            {
                                std::mem::swap(old, next);
                            }
                        }
                        prepared.compensation.generation = generation;
                    } else {
                        self.compensation_transition_frames =
                            prepared.compensation.output_latency as u64;
                    }
                    self.retired_routing = Some(previous);
                    self.return_retired_routing();
                }
                self.routing = Some(prepared);
            }
            EngineCommand::RejectRoutingUpdate { reason } => {
                if self.graph_edit_pending {
                    self.close_capture_on_failure();
                    self.compensation_valid = false;
                    self.transport.stop();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                }
                self.retire_event(EngineEvent::CompensationFailed { reason });
            }
            EngineCommand::ResumeDeviceReconfiguration { device, routing } => {
                self.restore_reconfigured_device(device);
                self.compensation_suspended = false;
                self.graph_edit_pending = false;
                self.compensation_valid = true;
                self.compensation_failure_reported = false;
                self.compensation_transition_frames = routing.compensation.output_latency as u64;
                self.retired_routing = self.routing.replace(routing);
                self.return_retired_routing();
            }
            EngineCommand::RejectDeviceReconfiguration { device, reason } => {
                self.restore_reconfigured_device(device);
                self.compensation_suspended = false;
                self.close_capture_on_failure();
                self.compensation_valid = false;
                self.transport.stop();
                self.retire_event(EngineEvent::CompensationFailed { reason });
                let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
            }
            EngineCommand::SetRouting(prepared) => {
                self.graph_edit_pending = false;
                self.compensation_transition_frames = if self.transport.is_playing() {
                    prepared.compensation.output_latency as u64
                } else {
                    0
                };
                self.compensation_valid = true;
                self.compensation_failure_reported = false;
                self.retired_routing = self.routing.replace(prepared);
                self.return_retired_routing();
            }
            cmd => return Err(cmd),
        }
        Ok(())
    }
}
