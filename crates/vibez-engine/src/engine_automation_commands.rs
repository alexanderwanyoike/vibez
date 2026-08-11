//! Audio-thread handlers for manual automation ownership and live gestures.

use vibez_core::automation::AutomationTarget;
use vibez_core::id::{EffectId, TrackId};
use vibez_core::perform::SwingOffset;
use vibez_core::perform::TrackMuteQuantization;

use crate::events::{AutomationGesturePhase, EngineEvent};
use crate::mixer::QueuedTrackMute;

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
        let cancelled = self
            .channel_mut(id)
            .is_some_and(|track| track.queued_mute.take().is_some());
        if cancelled {
            let _ = self
                .event_tx
                .push(EngineEvent::TrackMuteQueueCancelled { track_id: id });
        }
        self.apply_track_mute_at(id, muted, effective_at_samples);
    }

    fn apply_track_mute_at(&mut self, id: TrackId, muted: bool, effective_at_samples: u64) {
        let playing = self.transport.is_playing();
        let (changed, override_changed) = if let Some(track) = self.channel_mut(id) {
            track.queued_mute = None;
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

    pub(super) fn queue_track_mute(
        &mut self,
        track_id: TrackId,
        muted: bool,
        quantization: TrackMuteQuantization,
    ) {
        let now = self.effective_position();
        if quantization == TrackMuteQuantization::Immediate || !self.transport.is_playing() {
            self.apply_track_mute_at(track_id, muted, now);
            return;
        }

        let Some(track) = self.channel_mut(track_id) else {
            return;
        };
        if track.queued_mute.take().is_some() {
            let _ = self
                .event_tx
                .push(EngineEvent::TrackMuteQueueCancelled { track_id });
            return;
        }

        let effective_at_samples = if let Some(boundary) = quantization.musical_boundary() {
            boundary
                .beats()
                .map_or(now, |beats| self.next_grid_boundary(now, beats))
        } else {
            self.active_section
                .map(|active| {
                    now.saturating_add(
                        active
                            .length_samples
                            .saturating_sub(active.position_samples),
                    )
                })
                .unwrap_or(now)
        };
        if effective_at_samples <= now {
            let _ = self.event_tx.push(EngineEvent::TrackMuteQueued {
                track_id,
                muted,
                effective_at_samples: now,
            });
            self.apply_track_mute_at(track_id, muted, now);
            return;
        }
        let Some(track) = self.channel_mut(track_id) else {
            return;
        };
        track.queued_mute = Some(QueuedTrackMute {
            muted,
            effective_at_samples,
            end_of_section: quantization == TrackMuteQuantization::EndOfSection,
        });
        let _ = self.event_tx.push(EngineEvent::TrackMuteQueued {
            track_id,
            muted,
            effective_at_samples,
        });
    }

    pub(super) fn next_track_mute_boundary(&self) -> Option<u64> {
        self.tracks
            .iter()
            .filter_map(|track| track.queued_mute.map(|queued| queued.effective_at_samples))
            .min()
    }

    pub(super) fn apply_track_mutes_due(&mut self, at_samples: u64) {
        while let Some((track_id, queued)) = self.tracks.iter().find_map(|track| {
            track
                .queued_mute
                .filter(|queued| queued.effective_at_samples <= at_samples)
                .map(|queued| (track.id, queued))
        }) {
            self.apply_track_mute_at(track_id, queued.muted, queued.effective_at_samples);
        }
    }

    pub(super) fn apply_end_of_section_track_mutes(&mut self, at_samples: u64) {
        while let Some((track_id, muted)) = self.tracks.iter().find_map(|track| {
            track
                .queued_mute
                .filter(|queued| queued.end_of_section)
                .map(|queued| (track.id, queued.muted))
        }) {
            self.apply_track_mute_at(track_id, muted, at_samples);
        }
    }

    pub(super) fn cancel_queued_track_mutes(&mut self) {
        for track in &mut self.tracks {
            if track.queued_mute.take().is_some() {
                let _ = self
                    .event_tx
                    .push(EngineEvent::TrackMuteQueueCancelled { track_id: track.id });
            }
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
