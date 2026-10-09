//! Currency of plugin completions across device replacement and project replay.

use super::tracked_request::TrackedRequest;
use std::collections::HashMap;
use vibez_plugin_host::gui::PluginGuiKey;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PluginLoadToken {
    document: u64,
    request: u64,
}

#[derive(Default)]
pub(super) struct PluginLoadRequests {
    document: u64,
    requests: HashMap<PluginGuiKey, TrackedRequest>,
}

impl PluginLoadRequests {
    pub fn begin(&mut self, key: PluginGuiKey) -> PluginLoadToken {
        PluginLoadToken {
            document: self.document,
            request: self.requests.entry(key).or_default().begin(),
        }
    }

    pub fn finish(&mut self, key: PluginGuiKey, token: PluginLoadToken) -> bool {
        token.document == self.document
            && self
                .requests
                .get_mut(&key)
                .is_some_and(|request| request.finish(token.request))
    }

    pub fn cancel(&mut self, key: PluginGuiKey) {
        if let Some(request) = self.requests.get_mut(&key) {
            request.cancel();
        }
    }

    pub fn reset(&mut self) {
        // Recreated slots can reuse their IDs and per-slot request number.
        // The document epoch keeps their old completions stale after clearing.
        self.document = self.document.wrapping_add(1);
        self.requests.clear();
    }
}
