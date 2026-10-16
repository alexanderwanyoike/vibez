//! Native Filter cards on the visible device chain and their panel state.

use crate::domains::filter::FilterMsg;

use super::*;

impl App {
    pub(super) fn update_filter(&mut self, message: FilterMsg) -> Task<Message> {
        self.filter_panels
            .entry(message.effect_id())
            .update(message);
        Task::none()
    }
}
