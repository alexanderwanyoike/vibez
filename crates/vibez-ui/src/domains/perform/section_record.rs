#[cfg(test)]
pub use super::loop_record::RecordedLoopNote as RecordedSectionNote;
pub use super::loop_record::{
    LoopRecordCountIn as SectionRecordCountIn, LoopRecordMode as SectionRecordMode,
    LoopRecordMsg as SectionRecordMsg, LoopRecordPhase as SectionRecordPhase,
    LoopRecordQuantization as SectionRecordQuantization,
};
use super::{PerformAction, PerformState};
use vibez_core::id::SectionId;
pub type SectionRecordState = super::loop_record::LoopRecordState<SectionId>;
pub type SectionRecordStartRequest = super::loop_record::LoopRecordStartRequest<SectionId>;
pub(crate) type SectionRecordInput = super::loop_record::LoopRecordInput<SectionId>;
pub type CompletedSectionRecording = super::loop_record::CompletedLoopRecording<SectionId>;
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SectionRecordAction {
    Start(SectionRecordStartRequest),
    Stop,
}

impl PerformState {
    pub(super) fn update_section_record(&mut self, msg: SectionRecordMsg) -> PerformAction {
        match msg {
            SectionRecordMsg::SetCountIn(value) if !self.section_record.is_active() => {
                self.section_record.count_in = value;
            }
            SectionRecordMsg::SetMode(value) if !self.section_record.is_active() => {
                self.section_record.mode = value;
            }
            SectionRecordMsg::SetQuantization(value) if !self.section_record.is_active() => {
                self.section_record.quantization = value;
            }
            SectionRecordMsg::Toggle if self.section_record.is_active() => {
                if self.section_record.request_stop() {
                    return PerformAction {
                        section_record: Some(SectionRecordAction::Stop),
                        ..PerformAction::default()
                    };
                }
            }
            SectionRecordMsg::Toggle => {
                let Some(track_id) = self.instrument_target() else {
                    return PerformAction {
                        section_record_status: Some("Choose an Instrument Target before recording"),
                        ..PerformAction::default()
                    };
                };
                let section_id = if self.section_record.transport_playing {
                    self.playing_section
                } else {
                    self.selected_section
                };
                let Some(section_id) = section_id else {
                    return PerformAction {
                        section_record_status: Some(if self.section_record.transport_playing {
                            "Section Record requires a playing Section"
                        } else {
                            "Select a Section before recording"
                        }),
                        ..PerformAction::default()
                    };
                };
                let Some(section) = self.sections.by_id(section_id) else {
                    return PerformAction::default();
                };
                if let Some(request) = self.section_record.request_start(
                    section_id,
                    track_id,
                    !self.section_record.transport_playing,
                    section.length_beats,
                ) {
                    return PerformAction {
                        section_record: Some(SectionRecordAction::Start(request)),
                        ..PerformAction::default()
                    };
                }
            }
            _ => {}
        }
        PerformAction::default()
    }
}
