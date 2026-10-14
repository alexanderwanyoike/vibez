//! Identifies the console's native EQ and retains per-device analyser choices.

use crate::state::{UiEffect, UiTrack};
use vibez_core::effect::EffectType;
use vibez_core::id::{EffectId, TrackId};
use vibez_engine::effect_spectrum::{
    EffectSpectrumPosition, EffectSpectrumSample, EffectSpectrumTarget,
};

pub fn is_native_eq(effect: &UiEffect) -> bool {
    effect.effect_type == EffectType::Eq
        && effect.plugin_ref.is_none()
        && effect.plugin_name.is_none()
}

pub fn first_native_eq(track: &UiTrack) -> Option<&UiEffect> {
    track.effects.iter().find(|effect| is_native_eq(effect))
}

#[derive(Debug, Clone)]
pub enum EqMsg {
    Focus(TrackId, EffectId),
    SelectBand(TrackId, EffectId, usize),
    Source(TrackId, EffectId, EffectSpectrumPosition),
    Freeze(TrackId, EffectId),
}

impl EqMsg {
    pub fn destination(&self) -> (TrackId, EffectId) {
        match self {
            Self::Focus(track, effect)
            | Self::SelectBand(track, effect, _)
            | Self::Source(track, effect, _)
            | Self::Freeze(track, effect) => (*track, *effect),
        }
    }
}

#[derive(Default)]
pub struct EqPanel {
    pub selected_band: usize,
    pub post: bool,
    pub frozen: bool,
    pub spectrum: crate::spectrum::SpectrumState,
    pub last_sample_at: Option<std::time::Instant>,
}

impl EqPanel {
    pub fn retire_tap(&mut self) {
        self.last_sample_at = None;
        if !self.frozen {
            self.spectrum.reset();
        }
    }

    pub fn update(&mut self, message: EqMsg) {
        match message {
            EqMsg::Focus(..) => {}
            EqMsg::SelectBand(_, _, band) => self.selected_band = band.min(3),
            EqMsg::Source(_, _, source) => {
                self.post = source == EffectSpectrumPosition::Output;
                self.spectrum.reset();
            }
            EqMsg::Freeze(..) => {
                self.frozen = !self.frozen;
                if !self.frozen {
                    self.spectrum.reset();
                }
            }
        }
    }

    pub fn position(&self) -> EffectSpectrumPosition {
        if self.post {
            EffectSpectrumPosition::Output
        } else {
            EffectSpectrumPosition::Input
        }
    }
}

#[derive(Default)]
pub struct EqSpectrumRuntime {
    pub target: Option<EffectSpectrumTarget>,
    pub focus: Option<(TrackId, EffectId)>,
    pub generation: u64,
}

impl EqSpectrumRuntime {
    pub fn retarget(
        &mut self,
        wanted: Option<(TrackId, EffectId, EffectSpectrumPosition)>,
    ) -> bool {
        if self
            .target
            .map(|target| (target.track_id, target.effect_id, target.position))
            == wanted
        {
            return false;
        }
        self.generation = self.generation.wrapping_add(1);
        self.target = wanted.map(|(track_id, effect_id, position)| EffectSpectrumTarget {
            track_id,
            effect_id,
            position,
            generation: self.generation,
        });
        true
    }

    pub fn accept(&self, sample: EffectSpectrumSample) -> Option<f32> {
        self.target
            .filter(|target| target.generation == sample.generation)
            .map(|_| sample.value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::ProjectTrack;
    use vibez_dsp::effect::AudioEffect;

    fn eq() -> UiEffect {
        let effect = vibez_dsp::eq::EqEffect::new(48000.0);
        UiEffect {
            id: EffectId::new(),
            effect_type: EffectType::Eq,
            bypass: false,
            params: effect
                .param_descriptors()
                .iter()
                .map(|d| d.default)
                .collect(),
            descriptors: effect.param_descriptors(),
            plugin_ref: None,
            plugin_name: None,
            reconfiguration_failed: false,
            latency_samples: Some(0),
            has_plugin_gui: false,
            sidechains: vec![],
            inactive_sidechains: vec![],
            external_inputs: vec![],
        }
    }

    #[test]
    fn console_identity_follows_first_native_eq_after_reorder_and_removal() {
        let mut channel = ProjectTrack::new(TrackId::new(), "Channel".into(), 0);
        let mut hosted = eq();
        hosted.plugin_name = Some("Missing hosted EQ".into());
        let (first, second) = (eq(), eq());
        let (a, b) = (first.id, second.id);
        channel.effects = vec![hosted, first, second];
        assert_eq!(first_native_eq(&channel).unwrap().id, a);
        channel.effects.swap(1, 2);
        assert_eq!(first_native_eq(&channel).unwrap().id, b);
        channel.effects.remove(1);
        assert_eq!(first_native_eq(&channel).unwrap().id, a);
        channel.effects.remove(1);
        assert!(first_native_eq(&channel).is_none());
    }

    #[test]
    fn source_effect_and_track_changes_reject_queued_samples() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let mut runtime = EqSpectrumRuntime::default();
        assert!(runtime.retarget(Some((track, effect, EffectSpectrumPosition::Input))));
        let old = runtime.generation;
        assert!(!runtime.retarget(Some((track, effect, EffectSpectrumPosition::Input))));
        assert_eq!(
            runtime.accept(EffectSpectrumSample {
                generation: old,
                value: 0.25
            }),
            Some(0.25)
        );
        for target in [
            Some((track, effect, EffectSpectrumPosition::Output)),
            Some((track, EffectId::new(), EffectSpectrumPosition::Output)),
            Some((TrackId::new(), effect, EffectSpectrumPosition::Output)),
            None,
        ] {
            assert!(runtime.retarget(target));
            assert_eq!(
                runtime.accept(EffectSpectrumSample {
                    generation: old,
                    value: 0.25
                }),
                None
            );
        }
    }

    fn sounding_panel() -> EqPanel {
        let mut panel = EqPanel::default();
        let signal: Vec<_> = (0..2048)
            .map(|sample| (sample as f32 * 0.1).sin() * 0.25)
            .collect();
        panel.spectrum.ingest(&signal);
        panel.spectrum.analyse(48000.0);
        assert!(panel.spectrum.active);
        panel
    }

    fn assert_empty_analysis(panel: &mut EqPanel) {
        assert!(!panel.spectrum.active);
        assert!(panel
            .spectrum
            .bins
            .iter()
            .all(|value| *value == crate::spectrum::FLOOR_DB));
        panel.spectrum.analyse(48000.0);
        assert!(!panel.spectrum.active, "source history survived reset");
    }

    #[test]
    fn panel_clamps_band_selection_to_the_four_console_bands() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let mut panel = EqPanel::default();
        for (requested, expected) in [(0, 0), (2, 2), (3, 3), (4, 3), (usize::MAX, 3), (0, 0)] {
            panel.update(EqMsg::SelectBand(track, effect, requested));
            assert_eq!(panel.selected_band, expected);
        }
    }

    #[test]
    fn choosing_source_resolves_position_and_discards_previous_audio_history() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        for source in [
            EffectSpectrumPosition::Input,
            EffectSpectrumPosition::Output,
        ] {
            let mut panel = sounding_panel();
            panel.update(EqMsg::Source(track, effect, source));
            assert_eq!(panel.position(), source);
            assert_empty_analysis(&mut panel);
        }
    }

    #[test]
    fn freeze_preserves_display_and_unfreeze_discards_stored_audio_history() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let mut panel = sounding_panel();
        let bins = panel.spectrum.bins.clone();
        panel.update(EqMsg::Freeze(track, effect));
        assert!(panel.frozen);
        assert!(panel.spectrum.active);
        assert_eq!(panel.spectrum.bins, bins);
        panel.update(EqMsg::Freeze(track, effect));
        assert!(!panel.frozen);
        assert_empty_analysis(&mut panel);
    }

    #[test]
    fn analyser_focus_does_not_change_panel_choices_or_signal_history() {
        let (track, effect) = (TrackId::new(), EffectId::new());
        let mut panel = sounding_panel();
        panel.selected_band = 2;
        panel.post = true;
        panel.frozen = true;
        panel.last_sample_at = Some(std::time::Instant::now());
        let last = panel.last_sample_at;
        let bins = panel.spectrum.bins.clone();
        panel.update(EqMsg::Focus(track, effect));
        assert_eq!(panel.selected_band, 2);
        assert_eq!(panel.position(), EffectSpectrumPosition::Output);
        assert!(panel.frozen && panel.spectrum.active);
        assert_eq!(panel.spectrum.bins, bins);
        assert_eq!(panel.last_sample_at, last);
        panel.spectrum.analyse(48000.0);
        assert!(
            panel.spectrum.active,
            "focus discarded the stored audio window"
        );
    }

    #[test]
    fn retiring_a_tap_clears_unfrozen_history_but_keeps_an_explicitly_frozen_frame() {
        for frozen in [false, true] {
            let mut panel = sounding_panel();
            panel.selected_band = 2;
            panel.post = true;
            panel.frozen = frozen;
            panel.last_sample_at = Some(std::time::Instant::now());
            let bins = panel.spectrum.bins.clone();
            panel.retire_tap();
            assert!(panel.last_sample_at.is_none());
            assert_eq!(panel.selected_band, 2);
            assert!(panel.post);
            assert_eq!(panel.frozen, frozen);
            if frozen {
                assert!(panel.spectrum.active);
                assert_eq!(panel.spectrum.bins, bins);
            } else {
                assert_empty_analysis(&mut panel);
            }
        }
    }
}
