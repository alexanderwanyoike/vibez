//! What each native device card remembers between frames, kept per effect.

use std::collections::HashMap;

use vibez_core::id::EffectId;

pub(super) struct DevicePanels<T> {
    panels: HashMap<EffectId, T>,
    /// Shown for a card whose panel has not been touched yet.
    empty: T,
}

impl<T: Default> Default for DevicePanels<T> {
    fn default() -> Self {
        Self {
            panels: HashMap::new(),
            empty: T::default(),
        }
    }
}

impl<T: Default> DevicePanels<T> {
    pub fn panel(&self, effect_id: EffectId) -> &T {
        self.panels.get(&effect_id).unwrap_or(&self.empty)
    }

    pub fn entry(&mut self, effect_id: EffectId) -> &mut T {
        self.panels.entry(effect_id).or_default()
    }

    #[cfg(test)]
    pub fn contains(&self, effect_id: EffectId) -> bool {
        self.panels.contains_key(&effect_id)
    }

    /// Forgets panels whose device is no longer on any channel.
    pub fn retain_existing(&mut self, state: &crate::state::AppState) {
        self.panels.retain(|id, _| state.has_effect(*id));
    }

    pub fn clear(&mut self) {
        self.panels.clear();
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> {
        self.panels.values_mut()
    }
}
