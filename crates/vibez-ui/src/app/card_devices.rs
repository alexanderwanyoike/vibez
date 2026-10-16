//! Native cards with only an Advanced switch, and their panel state.

use crate::domains::device_card::CardMsg;

use super::*;

impl App {
    pub(super) fn update_card(&mut self, message: CardMsg) -> Task<Message> {
        self.card_panels.entry(message.effect_id()).update(message);
        Task::none()
    }
}
