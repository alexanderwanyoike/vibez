//! Typed audio-thread compensation commands helpers.

use super::*;

impl AudioEngine {
    pub(super) fn command_update_automation_routing(
        &mut self,
        prepared: Box<crate::routing::PreparedRouting>,
    ) {
        self.publish_compensation_plan(prepared);
    }

    pub(super) fn command_reject_routing_update(&mut self, reason: String) {
        if self.graph_edit_pending {
            self.close_capture_on_failure();
            self.compensation_valid = false;
            self.transport.stop();
            self.pending_playback_stop = true;
            self.flush_presentation();
        }
        self.retire_event(EngineEvent::CompensationFailed { reason });
    }

    pub(super) fn command_resume_device_reconfiguration(
        &mut self,
        device: crate::engine::reconfiguration::DeviceReconfiguration,
        routing: Box<crate::routing::PreparedRouting>,
    ) {
        if !self.handoff_is_current(&device) {
            self.retired_routing = Some(routing);
            self.return_retired_routing();
            self.retire_event(EngineEvent::DeviceReconfigurationRetired {
                device,
                reason: None,
            });
            return;
        }
        if !self.restore_reconfigured_device(device) {
            self.compensation_suspended = false;
            self.close_capture_on_failure();
            self.transport.stop();
            self.retired_routing = Some(routing);
            self.return_retired_routing();
            self.pending_playback_stop = true;
            self.flush_presentation();
            return;
        }
        self.compensation_suspended = false;
        self.graph_edit_pending = false;
        self.compensation_valid = true;
        self.publish_compensation_plan(routing);
    }

    pub(super) fn command_reject_device_reconfiguration(
        &mut self,
        device: crate::engine::reconfiguration::DeviceReconfiguration,
        reason: String,
    ) {
        if !self.handoff_is_current(&device) {
            self.retire_event(EngineEvent::DeviceReconfigurationRetired {
                device,
                reason: Some(reason),
            });
            return;
        }
        let _ = self.restore_reconfigured_device(device);
        self.compensation_suspended = false;
        self.close_capture_on_failure();
        self.compensation_valid = false;
        self.transport.stop();
        self.retire_event(EngineEvent::CompensationFailed { reason });
        self.pending_playback_stop = true;
        self.flush_presentation();
    }

    pub(super) fn command_set_routing(&mut self, prepared: Box<crate::routing::PreparedRouting>) {
        self.graph_edit_pending = false;
        self.compensation_valid = true;
        self.publish_compensation_plan(prepared);
    }
}
