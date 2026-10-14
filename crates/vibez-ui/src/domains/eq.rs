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
    SelectBand(TrackId, EffectId, usize),
    Source(TrackId, EffectId, EffectSpectrumPosition),
    Freeze(TrackId, EffectId),
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
}
