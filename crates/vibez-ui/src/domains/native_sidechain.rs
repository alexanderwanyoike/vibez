//! Projects validated sidechain choices and persisted unavailable routes for native panels.

use crate::{
    domains::devices::DevicesState,
    state::{ProjectTrack, UiEffect},
};
use vibez_core::{
    id::TrackId,
    routing::{ExternalInputId, SourceTap},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputChoice {
    pub id: ExternalInputId,
    pub label: String,
}
impl std::fmt::Display for InputChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceChoice {
    pub id: Option<TrackId>,
    pub label: String,
}
impl std::fmt::Display for SourceChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TapChoice(pub SourceTap);
impl std::fmt::Display for TapChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self.0 {
            SourceTap::BeforeEffects => "Pre FX",
            SourceTap::AfterEffects => "Post FX",
            SourceTap::AfterFader => "Post Fader",
        })
    }
}
pub struct NativeSidechain {
    pub inputs: Vec<InputChoice>,
    pub input: InputChoice,
    pub sources: Vec<SourceChoice>,
    pub source: SourceChoice,
    pub taps: Vec<TapChoice>,
    pub tap: TapChoice,
    pub assigned: bool,
    pub warning: Option<&'static str>,
    pub raw_peak: f32,
}

pub fn presentation(
    effect: &UiEffect,
    sources: &[ProjectTrack],
    buses: &[ProjectTrack],
    state: &DevicesState,
    wanted: ExternalInputId,
) -> Option<NativeSidechain> {
    let mut inputs: Vec<_> = effect
        .external_inputs
        .iter()
        .filter(|input| input.supported())
        .map(|input| InputChoice {
            id: input.id,
            label: input.name.clone(),
        })
        .collect();
    for route in &effect.sidechains {
        if !inputs.iter().any(|input| input.id == route.input_id) {
            inputs.push(InputChoice {
                id: route.input_id,
                label: format!(
                    "{} (unavailable)",
                    if route.input_name.is_empty() {
                        format!("Input {}", route.input_id.0)
                    } else {
                        route.input_name.clone()
                    }
                ),
            });
        }
    }
    let input = inputs
        .iter()
        .find(|input| input.id == wanted)
        .or_else(|| inputs.first())?
        .clone();
    let route = effect
        .sidechains
        .iter()
        .find(|route| route.input_id == input.id);
    let allowed = state
        .sidechain_choices
        .get(&(effect.id, input.id))
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut choices = vec![SourceChoice {
        id: None,
        label: "Internal".into(),
    }];
    choices.extend(allowed.iter().filter_map(|option| {
        sources
            .iter()
            .chain(buses)
            .find(|source| source.id == option.source)
            .map(|source| SourceChoice {
                id: Some(source.id),
                label: source.name.clone(),
            })
    }));
    let source = route.map_or_else(
        || choices[0].clone(),
        |route| SourceChoice {
            id: Some(route.source),
            label: sources
                .iter()
                .chain(buses)
                .find(|source| source.id == route.source)
                .map_or_else(
                    || format!("{} (missing)", route.source_name),
                    |source| source.name.clone(),
                ),
        },
    );
    let unavailable = route.is_some_and(|route| {
        !effect
            .external_inputs
            .iter()
            .any(|input| route.matches(input))
    });
    let inactive = effect.inactive_sidechains.contains(&input.id);
    let taps = route
        .and_then(|route| allowed.iter().find(|choice| choice.source == route.source))
        .map(|choice| choice.taps.iter().copied().map(TapChoice).collect())
        .unwrap_or_default();
    let raw = state
        .sidechain_meters
        .get(&(effect.id, input.id))
        .copied()
        .unwrap_or_default();
    Some(NativeSidechain {
        inputs,
        input,
        sources: choices,
        source,
        taps,
        tap: TapChoice(route.map_or(SourceTap::AfterEffects, |route| route.tap)),
        assigned: route.is_some(),
        warning: if unavailable {
            Some("Input unavailable")
        } else if inactive {
            Some("Route inactive")
        } else {
            None
        },
        raw_peak: raw.0.max(raw.1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use vibez_core::{
        effect::EffectType,
        id::EffectId,
        routing::{ExternalInputDescriptor, InputSourceChoice, SidechainAssignment},
    };
    fn effect() -> UiEffect {
        UiEffect {
            reconfiguration_failed: false,
            latency_samples: Some(0),
            id: EffectId::new(),
            effect_type: EffectType::Compressor,
            bypass: false,
            params: vec![],
            descriptors: &[],
            plugin_name: None,
            has_plugin_gui: false,
            plugin_ref: None,
            sidechains: vec![],
            inactive_sidechains: vec![],
            external_inputs: vec![ExternalInputDescriptor {
                id: ExternalInputId(0),
                name: "Sidechain".into(),
                channels: 2,
            }],
        }
    }
    #[test]
    fn native_choices_are_exactly_the_cached_validated_sources_and_taps() {
        let effect = effect();
        let source = ProjectTrack::new(TrackId::new(), "Kick".into(), 3);
        let excluded = ProjectTrack::new(TrackId::new(), "Feedback risk".into(), 4);
        let mut state = DevicesState::default();
        state.sidechain_choices.insert(
            (effect.id, ExternalInputId(0)),
            vec![InputSourceChoice {
                source: source.id,
                taps: vec![SourceTap::BeforeEffects],
            }],
        );
        let model = presentation(
            &effect,
            &[source.clone(), excluded],
            &[],
            &state,
            ExternalInputId(0),
        )
        .unwrap();
        assert_eq!(model.sources.len(), 2);
        assert_eq!(model.sources[0].id, None);
        assert_eq!(model.sources[1].id, Some(source.id));
        assert!(!model.assigned);
    }
    #[test]
    fn missing_unavailable_and_inactive_routes_remain_visible_and_disconnectable() {
        let mut effect = effect();
        let missing = TrackId::new();
        effect.sidechains.push(SidechainAssignment {
            input_id: ExternalInputId(7),
            input_name: "Old detector".into(),
            source: missing,
            source_name: "Lost kick".into(),
            tap: SourceTap::AfterFader,
        });
        let state = DevicesState::default();
        let model = presentation(&effect, &[], &[], &state, ExternalInputId(7)).unwrap();
        assert_eq!(model.input.id, ExternalInputId(7));
        assert_eq!(model.source.id, Some(missing));
        assert_eq!(model.source.label, "Lost kick (missing)");
        assert_eq!(model.warning, Some("Input unavailable"));
        assert!(model.assigned);
        assert!(model.taps.is_empty());
        effect.sidechains[0].input_id = ExternalInputId(0);
        effect.sidechains[0].input_name = "Sidechain".into();
        effect.inactive_sidechains.push(ExternalInputId(0));
        let model = presentation(&effect, &[], &[], &state, ExternalInputId(0)).unwrap();
        assert_eq!(model.warning, Some("Route inactive"));
        assert_eq!(model.sources[0].id, None);
    }
}
