//! The dialog asking how a dropped multi-track MIDI file lands.

use iced::widget::{button, column, horizontal_space, row, text};
use iced::Element;

use crate::domains::midi_file_import::{MidiFileLayout, MidiFileLayoutChoice};
use crate::message::Message;
use crate::theme as th;

use super::views_dialog::{dialog_choice, dialog_overlay};
use super::*;

impl App {
    pub(super) fn view_midi_file_layout_overlay<'a>(
        &'a self,
        choice: &'a MidiFileLayoutChoice,
    ) -> Element<'a, Message> {
        let tracks = &choice.import.file.tracks;
        let names: Vec<_> = tracks
            .iter()
            .enumerate()
            .map(|(index, track)| {
                track
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("Track {}", index + 1))
            })
            .collect();
        let drums = tracks.iter().filter(|track| track.on_drum_channel).count();
        let split_detail = match drums {
            0 => format!("{} new MIDI tracks", tracks.len()),
            drums if drums == tracks.len() => {
                format!("{} new MIDI tracks, each on a Drum Rack", tracks.len())
            }
            drums => format!(
                "{} new MIDI tracks, {drums} of them on a Drum Rack",
                tracks.len()
            ),
        };
        let cancel = button(text("Cancel").size(12).color(th::text()))
            .on_press(Message::ChooseMidiFileLayout(None))
            .padding([7, 14]);
        let card = column![
            text(format!("Import {}", choice.import.name))
                .size(18)
                .color(th::text()),
            text(names.join(", ")).size(11).color(th::text_dim()),
            row![
                dialog_choice(
                    "One Clip".into(),
                    "Every track's notes together where you dropped".into(),
                    false,
                    Message::ChooseMidiFileLayout(Some(MidiFileLayout::OneClip)),
                ),
                dialog_choice(
                    "A track each".into(),
                    split_detail,
                    false,
                    Message::ChooseMidiFileLayout(Some(MidiFileLayout::TrackPerFileTrack)),
                ),
            ]
            .spacing(8),
            row![horizontal_space(), cancel].align_y(iced::Alignment::Center),
        ]
        .spacing(12);
        dialog_overlay(card, 460.0, Message::ChooseMidiFileLayout(None))
    }
}
