//! UI-owned guards prevent sample voices from destroying their last decoded buffer.

use std::collections::HashMap;
use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;
use vibez_instruments::InstrumentMediaOwners;

#[derive(Default)]
pub(super) struct SampleRetirement {
    guards: HashMap<usize, Arc<DecodedAudio>>,
}

impl SampleRetirement {
    pub(super) fn retire(&mut self, owners: InstrumentMediaOwners) {
        if let Some(sample) = owners.sample {
            // Deduplicate before counting: two guards would keep each other alive.
            self.guards
                .entry(Arc::as_ptr(&sample) as usize)
                .or_insert(sample);
        }
    }

    pub(super) fn reap(&mut self) {
        // Callback sources clone existing strong pad/voice owners, never Weak refs.
        self.guards
            .retain(|_, sample| Arc::strong_count(sample) > 1);
    }
}
