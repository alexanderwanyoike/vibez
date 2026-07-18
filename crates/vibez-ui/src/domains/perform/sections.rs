use std::sync::Arc;

use vibez_core::id::{ClipId, LaneId, SectionId, TrackId};
use vibez_project::SectionLaunchQuantization;

use crate::state::ArrangementTimeline;

pub const DEFAULT_SECTION_LENGTH_BEATS: f64 = 16.0;
pub const MIN_SECTION_LENGTH_BEATS: f64 = 4.0;
pub const MAX_SECTION_LENGTH_BEATS: f64 = 1024.0;

#[derive(Debug, Clone)]
pub struct Section {
    pub id: SectionId,
    pub slot: u16,
    pub name: String,
    pub length_beats: f64,
    pub launch_quantization: SectionLaunchQuantization,
    pub looping: bool,
    pub timeline: Arc<ArrangementTimeline>,
}

impl Section {
    pub fn new(slot: u16) -> Self {
        Self {
            id: SectionId::new(),
            slot,
            name: format!("Section {:02}", slot + 1),
            length_beats: DEFAULT_SECTION_LENGTH_BEATS,
            launch_quantization: SectionLaunchQuantization::default(),
            looping: true,
            timeline: Arc::new(ArrangementTimeline::default()),
        }
    }

    pub fn duplicate_to(&self, slot: u16) -> Self {
        let mut timeline = self.timeline.as_ref().clone();
        for content in timeline.by_track.values_mut() {
            for clip in &mut content.clips {
                clip.id = ClipId::new();
            }
            for clip in &mut content.note_clips {
                clip.id = ClipId::new();
                clip.selected_notes.clear();
            }
            for lane in &mut content.automation {
                lane.id = LaneId::new();
            }
        }
        Self {
            id: SectionId::new(),
            slot,
            name: format!("{} Copy", self.name),
            length_beats: self.length_beats,
            launch_quantization: self.launch_quantization,
            looping: self.looping,
            timeline: Arc::new(timeline),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SectionStore {
    pub sections: Vec<Section>,
}

impl SectionStore {
    pub fn at_slot(&self, slot: u16) -> Option<&Section> {
        self.sections.iter().find(|section| section.slot == slot)
    }

    pub fn by_id(&self, id: SectionId) -> Option<&Section> {
        self.sections.iter().find(|section| section.id == id)
    }

    pub fn by_id_mut(&mut self, id: SectionId) -> Option<&mut Section> {
        self.sections.iter_mut().find(|section| section.id == id)
    }

    pub fn insert(&mut self, section: Section) {
        debug_assert!(self.at_slot(section.slot).is_none());
        self.sections.push(section);
        self.sections.sort_by_key(|section| section.slot);
    }

    pub fn remove(&mut self, id: SectionId) -> Option<Section> {
        let index = self.sections.iter().position(|section| section.id == id)?;
        Some(self.sections.remove(index))
    }

    pub fn remove_track(&mut self, track_id: TrackId) {
        for section in &mut self.sections {
            Arc::make_mut(&mut section.timeline).remove(track_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::{TrackTimelineContent, UiNoteClip};

    #[test]
    fn duplicate_remints_every_editable_identity() {
        let track_id = TrackId::new();
        let mut source = Section::new(0);
        let note_id = ClipId::new();
        let lane = vibez_core::automation::AutomationLane::new(
            vibez_core::automation::AutomationTarget::TrackGain,
        );
        let original_lane_id = lane.id;
        Arc::make_mut(&mut source.timeline).by_track.insert(
            track_id,
            TrackTimelineContent {
                note_clips: vec![UiNoteClip {
                    id: note_id,
                    name: "Pattern".into(),
                    position_beats: 0.0,
                    duration_beats: 4.0,
                    notes: Vec::new(),
                    selected_notes: [0].into_iter().collect(),
                    loop_enabled: false,
                    loop_start_beats: 0.0,
                    loop_end_beats: 0.0,
                }],
                automation: vec![lane],
                ..TrackTimelineContent::default()
            },
        );

        let duplicate = source.duplicate_to(7);

        assert_ne!(duplicate.id, source.id);
        assert_ne!(
            duplicate.timeline.get(track_id).unwrap().note_clips[0].id,
            note_id
        );
        assert_ne!(
            duplicate.timeline.get(track_id).unwrap().automation[0].id,
            original_lane_id
        );
        assert!(duplicate.timeline.get(track_id).unwrap().note_clips[0]
            .selected_notes
            .is_empty());
    }
}
