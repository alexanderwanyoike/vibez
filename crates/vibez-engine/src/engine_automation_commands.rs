//! Audio-thread handlers for manual automation ownership and live gestures.

use vibez_core::automation::AutomationTarget;
use vibez_core::id::{EffectId, TrackId};
use vibez_core::perform::SwingOffset;

use crate::events::{AutomationGesturePhase, EngineEvent};

use super::AudioEngine;

impl AudioEngine {
    pub(super) fn set_track_gain(&mut self, id: TrackId, gain: f32) {
        if let Some(track) = self.channel_mut(id) {
            track.gain = gain;
        }
    }

    pub(super) fn set_track_pan(&mut self, id: TrackId, pan: f32) {
        if let Some(track) = self.channel_mut(id) {
            track.pan = pan.clamp(0.0, 1.0);
        }
    }

    pub(super) fn set_track_swing_offset(&mut self, id: TrackId, offset: Option<SwingOffset>) {
        if let Some(track) = self.tracks.iter_mut().find(|track| track.id == id) {
            track.swing_offset = offset;
        }
    }

    pub(super) fn set_effect_param(
        &mut self,
        track_id: TrackId,
        effect_id: EffectId,
        param_index: usize,
        value: f32,
    ) {
        if let Some(track) = self.channel_mut(track_id) {
            if let Some(slot) = track.effects.iter_mut().find(|slot| slot.id == effect_id) {
                slot.effect.set_param(param_index, value);
            }
        }
    }

    pub(super) fn set_track_mute(&mut self, id: TrackId, muted: bool) {
        let effective_at_samples = self.effective_position();
        let playing = self.transport.is_playing();
        let (changed, override_changed) = if let Some(track) = self.channel_mut(id) {
            let target = AutomationTarget::TrackMute;
            let override_changed =
                track.has_automation_target(target) && track.set_automation_override(target, true);
            track.set_manual_mute(muted, !playing);
            (true, override_changed)
        } else {
            (false, false)
        };
        if changed {
            let _ = self.event_tx.push(EngineEvent::TrackMuteChanged {
                track_id: id,
                muted,
                effective_at_samples,
            });
        }
        if override_changed {
            let _ = self.event_tx.push(EngineEvent::AutomationOverrideChanged {
                track_id: id,
                target: AutomationTarget::TrackMute,
                overridden: true,
            });
        }
    }

    pub(super) fn set_automation_override(
        &mut self,
        track_id: TrackId,
        target: AutomationTarget,
        overridden: bool,
    ) {
        let changed = self
            .channel_mut(track_id)
            .is_some_and(|track| track.set_automation_override(target, overridden));
        if changed {
            let _ = self.event_tx.push(EngineEvent::AutomationOverrideChanged {
                track_id,
                target,
                overridden,
            });
        }
    }

    pub(super) fn update_automation_gesture(
        &mut self,
        track_id: TrackId,
        target: AutomationTarget,
        normalized_value: f32,
        begin: bool,
    ) {
        self.set_automation_override(track_id, target, true);
        let _ = self.event_tx.push(EngineEvent::AutomationGestureChanged {
            track_id,
            target,
            normalized_value: normalized_value.clamp(0.0, 1.0),
            phase: if begin {
                AutomationGesturePhase::Begin
            } else {
                AutomationGesturePhase::Update
            },
            effective_at_samples: self.effective_position(),
        });
    }

    pub(super) fn end_automation_gesture(&mut self, track_id: TrackId, target: AutomationTarget) {
        let section_active = self.active_section.is_some();
        let beat = if let Some(active) = self.active_section {
            self.samples_to_automation_beat(active.position_samples)
        } else {
            self.samples_to_automation_beat(self.effective_position())
        };
        let normalized_value = self
            .tracks
            .iter()
            .find(|track| track.id == track_id)
            .map(|track| track.normalized_target_value(target, beat, section_active))
            .or_else(|| {
                if self.master.id == track_id {
                    Some(self.master.normalized_target_value(target, beat, false))
                } else {
                    self.buses
                        .iter()
                        .find(|track| track.id == track_id)
                        .map(|track| track.normalized_target_value(target, beat, false))
                }
            });
        self.set_automation_override(track_id, target, false);
        if let Some(normalized_value) = normalized_value {
            let _ = self.event_tx.push(EngineEvent::AutomationGestureChanged {
                track_id,
                target,
                normalized_value,
                phase: AutomationGesturePhase::End,
                effective_at_samples: self.effective_position(),
            });
        }
    }

    fn samples_to_automation_beat(&self, samples: u64) -> f64 {
        let bpm = self.transport.bpm();
        if bpm > 0.0 {
            samples as f64 * bpm / (self.sample_rate as f64 * 60.0)
        } else {
            0.0
        }
    }
}
