use super::*;

impl AudioEngine {
    // Returning the inline owner avoids allocating a command box in the callback.
    #[allow(clippy::result_large_err)]
    pub(super) fn handle_compensation_command(
        &mut self,
        cmd: EngineCommand,
    ) -> Result<(), EngineCommand> {
        match cmd {
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
                if !self.handoff_is_current(&device) {
                    self.retired_routing = Some(routing);
                    self.return_retired_routing();
                    self.retire_event(EngineEvent::DeviceReconfigurationRetired {
                        device,
                        reason: None,
                    });
                    return Ok(());
                }
                if !self.restore_reconfigured_device(device) {
                    self.compensation_suspended = false;
                    self.close_capture_on_failure();
                    self.transport.stop();
                    self.retired_routing = Some(routing);
                    self.return_retired_routing();
                    let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
                    return Ok(());
                }
                self.compensation_suspended = false;
                self.graph_edit_pending = false;
                self.compensation_valid = true;
                self.compensation_failure_reported = false;
                self.retired_routing = self.routing.replace(routing);
                self.return_retired_routing();
            }
            EngineCommand::RejectDeviceReconfiguration { device, reason } => {
                if !self.handoff_is_current(&device) {
                    self.retire_event(EngineEvent::DeviceReconfigurationRetired {
                        device,
                        reason: Some(reason),
                    });
                    return Ok(());
                }
                let _ = self.restore_reconfigured_device(device);
                self.compensation_suspended = false;
                self.close_capture_on_failure();
                self.compensation_valid = false;
                self.transport.stop();
                self.retire_event(EngineEvent::CompensationFailed { reason });
                let _ = self.event_tx.push(EngineEvent::PlaybackStopped);
            }
            EngineCommand::SetRouting(prepared) => {
                self.graph_edit_pending = false;
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
