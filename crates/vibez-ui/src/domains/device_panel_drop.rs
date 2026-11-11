//! What audio dropped from the Browser on a channel's device panel, outside
//! any instrument card, does to that channel.

use vibez_core::midi::InstrumentKind;

use crate::state::ProjectTrack;

/// The outcome of a drop on the device panel outside any instrument card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePanelDrop {
    /// A MIDI track with no instrument gains a Sampler holding the audio.
    AddSampler,
    /// The named instrument gives way to a Sampler holding the audio.
    ReplaceWithSampler { instrument: String },
    /// The track's Sampler loads the audio and keeps its settings.
    LoadIntoExisting,
}

impl DevicePanelDrop {
    /// What a drop on `channel`'s panel does, or `None` for a channel that
    /// takes no instrument.
    pub fn for_channel(channel: &ProjectTrack) -> Option<Self> {
        if super::devices::instrument_refusal(Some(channel), InstrumentKind::Sampler.name())
            .is_some()
        {
            return None;
        }
        if !channel.has_instrument {
            return Some(Self::AddSampler);
        }
        if super::sampler::is_native_sampler(channel) {
            return Some(Self::LoadIntoExisting);
        }
        let instrument = channel
            .plugin_instrument_name
            .clone()
            .or_else(|| {
                channel
                    .plugin_instrument_ref
                    .as_ref()
                    .map(|plugin| plugin.name.clone())
            })
            .or_else(|| channel.instrument_kind.map(|kind| kind.name().to_string()))
            .unwrap_or_else(|| "the instrument".to_string());
        Some(Self::ReplaceWithSampler { instrument })
    }

    /// The status once the Sampler holds `sample`.
    pub fn loaded_status(&self, sample: &str) -> String {
        match self {
            Self::AddSampler => format!("Added a Sampler holding '{sample}'"),
            Self::ReplaceWithSampler { instrument } => {
                format!("Replaced {instrument} with a Sampler holding '{sample}'")
            }
            Self::LoadIntoExisting => format!("Imported '{sample}' to Sampler"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::id::TrackId;
    use vibez_core::midi::TrackKind;

    fn midi_track() -> ProjectTrack {
        ProjectTrack::new_instrument(TrackId::new(), "MIDI 1".into(), TrackKind::Midi, 0)
    }

    #[test]
    fn a_midi_track_without_an_instrument_gains_a_sampler() {
        assert_eq!(
            DevicePanelDrop::for_channel(&midi_track()),
            Some(DevicePanelDrop::AddSampler)
        );
    }

    #[test]
    fn a_built_in_instrument_is_replaced_by_name() {
        let mut track = midi_track();
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::FmSynth);
        assert_eq!(
            DevicePanelDrop::for_channel(&track),
            Some(DevicePanelDrop::ReplaceWithSampler {
                instrument: "FM Synth".into()
            })
        );
    }

    #[test]
    fn a_sampler_loads_the_drop_and_keeps_its_settings() {
        let mut track = midi_track();
        track.has_instrument = true;
        track.instrument_kind = Some(InstrumentKind::Sampler);
        assert_eq!(
            DevicePanelDrop::for_channel(&track),
            Some(DevicePanelDrop::LoadIntoExisting)
        );
    }

    #[test]
    fn a_plugin_instrument_is_replaced_by_its_plugin_name() {
        let mut track = midi_track();
        track.has_instrument = true;
        track.plugin_instrument_name = Some("Surge XT".into());
        assert_eq!(
            DevicePanelDrop::for_channel(&track),
            Some(DevicePanelDrop::ReplaceWithSampler {
                instrument: "Surge XT".into()
            })
        );
    }

    #[test]
    fn audio_tracks_and_the_master_take_no_panel_drop() {
        let audio = ProjectTrack::new(TrackId::new(), "Audio 1".into(), 0);
        assert_eq!(DevicePanelDrop::for_channel(&audio), None);
        assert_eq!(
            DevicePanelDrop::for_channel(&crate::state::new_master_track()),
            None
        );
    }

    #[test]
    fn the_loaded_status_says_what_the_drop_did() {
        assert_eq!(
            DevicePanelDrop::AddSampler.loaded_status("kick.wav"),
            "Added a Sampler holding 'kick.wav'"
        );
        assert_eq!(
            DevicePanelDrop::ReplaceWithSampler {
                instrument: "Surge XT".into()
            }
            .loaded_status("kick.wav"),
            "Replaced Surge XT with a Sampler holding 'kick.wav'"
        );
        assert_eq!(
            DevicePanelDrop::LoadIntoExisting.loaded_status("kick.wav"),
            "Imported 'kick.wav' to Sampler"
        );
    }
}
