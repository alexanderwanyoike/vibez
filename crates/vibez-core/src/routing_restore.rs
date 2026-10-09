//! Restore compatible inputs without activating feedback or losing assignments.

use super::*;

pub fn resolve_restored(
    channels: &[RoutingChannel],
    previous_active: Option<&[RoutingChannel]>,
) -> Result<Vec<RoutingChannel>, RoutingError> {
    let mut active = channels.to_vec();
    let mut pending = Vec::new();
    for (channel_index, channel) in channels.iter().enumerate() {
        for (effect_index, effect) in channel.effects.iter().enumerate() {
            for route in &effect.assignments {
                if effect.inactive_inputs.contains(&route.input_id)
                    || !effect.inputs.iter().any(|input| route.matches(input))
                {
                    continue;
                }
                let established = previous_active.is_some_and(|previous| {
                    previous
                        .iter()
                        .find(|old| old.id == channel.id)
                        .and_then(|old| old.effects.iter().find(|old| old.id == effect.id))
                        .is_some_and(|old| {
                            !old.inactive_inputs.contains(&route.input_id)
                                && old.inputs.iter().any(|input| route.matches(input))
                                && old.assignments.iter().any(|old_route| {
                                    old_route.input_id == route.input_id
                                        && old_route.source == route.source
                                        && old_route.tap == route.tap
                                })
                        })
                });
                active[channel_index].effects[effect_index]
                    .inactive_inputs
                    .push(route.input_id);
                pending.push((
                    !established,
                    channel.id.raw(),
                    effect.id.raw(),
                    route.input_id.0,
                    channel_index,
                    effect_index,
                    route.input_id,
                ));
            }
        }
    }
    RoutingGraph::prepare(&active)?;
    // Established edges retain priority. Persisted inactive IDs make later
    // reopening independent of asynchronous plugin completion order.
    pending.sort_by_key(|entry| (entry.0, entry.1, entry.2, entry.3));
    for (_, _, _, _, channel, effect, input) in pending {
        active[channel].effects[effect]
            .inactive_inputs
            .retain(|id| *id != input);
        match RoutingGraph::prepare(&active) {
            Ok(_) => {}
            Err(RoutingError::FeedbackLoop) => {
                active[channel].effects[effect].inactive_inputs.push(input);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(active)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receiver(id: TrackId, source: TrackId) -> RoutingChannel {
        RoutingChannel {
            id,
            is_bus: false,
            sends: vec![],
            effects: vec![RoutingEffect {
                id: EffectId::new(),
                inactive_inputs: vec![],
                inputs: vec![ExternalInputDescriptor {
                    id: ExternalInputId(1),
                    name: "Detector".into(),
                    channels: 2,
                }],
                assignments: vec![SidechainAssignment {
                    input_id: ExternalInputId(1),
                    input_name: "Detector".into(),
                    source,
                    source_name: "Source".into(),
                    tap: SourceTap::AfterEffects,
                }],
            }],
        }
    }

    #[test]
    fn restored_feedback_retains_established_route_and_assignment() {
        let a = TrackId::new();
        let b = TrackId::new();
        let channels = vec![receiver(a, b), receiver(b, a)];
        let mut previous = channels.clone();
        previous[0].effects[0].inputs.clear();
        let active = resolve_restored(&channels, Some(&previous)).unwrap();
        assert_eq!(active[0].effects[0].inactive_inputs, [ExternalInputId(1)]);
        assert!(active[1].effects[0].inactive_inputs.is_empty());
        assert_eq!(
            active[0].effects[0].assignments,
            channels[0].effects[0].assignments
        );
        assert!(RoutingGraph::prepare(&active).is_ok());
        assert_eq!(resolve_restored(&active, None).unwrap(), active);
        let reordered = vec![active[1].clone(), active[0].clone()];
        assert_eq!(resolve_restored(&reordered, None).unwrap(), reordered);
    }

    #[test]
    fn deliberate_valid_tap_can_replace_a_retained_inactive_route() {
        let id = TrackId::new();
        let mut channel = receiver(id, id);
        channel.effects[0].inactive_inputs.push(ExternalInputId(1));
        assert_eq!(
            valid_input_taps(
                &[channel.clone()],
                id,
                channel.effects[0].id,
                ExternalInputId(1),
                id
            ),
            [SourceTap::BeforeEffects]
        );
    }
}
