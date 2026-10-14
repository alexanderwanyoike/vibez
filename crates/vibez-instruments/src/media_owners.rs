//! Native sample owners returned for destruction outside audio processing.

use std::sync::Arc;
use vibez_core::audio_buffer::DecodedAudio;

/// Audio can still be referenced by sounding or inactive voices. The owner
/// thread must deduplicate returned audio guards and retain one per allocation
/// until no other strong references remain. Names can be destroyed immediately.
#[must_use = "return media owners to the UI before destroying them"]
#[derive(Default)]
pub struct InstrumentMediaOwners {
    pub sample: Option<Arc<DecodedAudio>>,
    pub name: Option<String>,
}

impl InstrumentMediaOwners {
    pub fn incoming(sample: Arc<DecodedAudio>, name: String) -> Self {
        Self {
            sample: Some(sample),
            name: Some(name),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sample.is_none() && self.name.is_none()
    }
}

impl PartialEq for InstrumentMediaOwners {
    fn eq(&self, other: &Self) -> bool {
        let same_sample = match (&self.sample, &other.sample) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        };
        same_sample && self.name == other.name
    }
}

impl std::fmt::Debug for InstrumentMediaOwners {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InstrumentMediaOwners")
            .field(
                "sample",
                &self.sample.as_ref().map(Arc::as_ptr),
            )
            .field("name", &self.name)
            .finish()
    }
}
