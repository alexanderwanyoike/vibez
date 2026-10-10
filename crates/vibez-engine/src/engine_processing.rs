//! Lazy cached device diagnostics shared by live and offline processing.

use super::*;

pub(super) fn processing_errors<'a>(
    tracks: &'a mut [EngineTrack],
    buses: &'a mut [EngineTrack],
    master: &'a mut EngineTrack,
) -> impl Iterator<Item = (TrackId, Option<vibez_core::id::EffectId>, &'static str)> + 'a {
    tracks
        .iter_mut()
        .chain(buses.iter_mut())
        .chain(std::iter::once(master))
        .flat_map(|track| {
            let id = track.id;
            let instrument = track
                .instrument
                .as_mut()
                .into_iter()
                .filter_map(move |instrument| {
                    instrument
                        .take_processing_error()
                        .map(|reason| (id, None, reason))
                });
            let effects = track.effects.iter_mut().filter_map(move |slot| {
                slot.effect
                    .take_processing_error()
                    .map(|reason| (id, Some(slot.id), reason))
            });
            instrument.chain(effects)
        })
}

impl AudioEngine {
    pub(super) fn flush_processing_errors(&mut self) {
        let mut errors = processing_errors(&mut self.tracks, &mut self.buses, &mut self.master);
        // The UI can only free capacity. This is the sole producer, so a
        // cached cause is consumed only when its following push must succeed.
        while !self.event_tx.is_full() {
            let Some((track_id, effect_id, reason)) = errors.next() else {
                break;
            };
            let _ = self.event_tx.push(EngineEvent::DeviceProcessingFailed {
                track_id,
                effect_id,
                reason,
            });
        }
    }

    /// Offline renderers without a UI event consumer pull a cached cause directly.
    pub fn take_device_processing_error(
        &mut self,
    ) -> Option<(TrackId, Option<vibez_core::id::EffectId>, &'static str)> {
        processing_errors(&mut self.tracks, &mut self.buses, &mut self.master).next()
    }
}
