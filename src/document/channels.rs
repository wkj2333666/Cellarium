//! Pure experiment transforms for the channel and growth lifecycle.
//!
//! These were previously inlined in the terminal Workbench state. They are kept
//! free of editor state so the Workbench and the GUI document controller share
//! one implementation of the rules.

use std::collections::BTreeSet;

use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId, KernelSlot, UpdateMode};
use crate::sim::ruleset::{BindingKey, KernelSpatialDefinition, RuleBinding, RuleSet, RuleSetId};

/// Result of adding a channel: the new draft, the added channel, and the kernel
/// an editor should select so the new binding is immediately editable.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelAddition {
    pub spec: ExperimentSpec,
    pub channel: ChannelId,
    pub selected_kernel: Option<KernelId>,
}

/// Append a channel and everything it needs to be simulated.
pub fn add_channel(draft: &ExperimentSpec) -> Result<ChannelAddition, String> {
    let mut next = draft.clone();
    let ordinal = next
        .channels
        .iter()
        .map(|channel| channel.id.0)
        .max()
        .unwrap_or(0)
        .checked_add(2)
        .ok_or_else(|| "channel name ordinal exhausted".to_string())?;
    let name = format!("channel_{ordinal}");
    let id = next.add_channel(name, false);

    let selected_kernel = if next.rules.is_empty() {
        let kernel_id = KernelId(
            next.kernels
                .iter()
                .map(|kernel| kernel.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| "kernel id exhausted".to_string())?,
        );
        let symbol = format!("k{}", kernel_id.0);
        next.kernels
            .push(KernelSlot::identity(kernel_id, symbol, id, id));
        let growth = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == id)
            .ok_or_else(|| "new channel growth is missing".to_string())?;
        growth.kernel_inputs = vec![kernel_id];
        Some(kernel_id)
    } else {
        next.growth.retain(|growth| growth.target != id);
        let rule_set_id = RuleSetId(
            next.rules
                .sets
                .iter()
                .map(|rule| rule.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| "rule-set id exhausted".to_string())?,
        );
        let mut rule_set = RuleSet::identity(rule_set_id, id);
        if next.tiling.is_some() {
            let planes = next
                .basis_ids()
                .iter()
                .map(|basis| {
                    (
                        *basis,
                        BasisWeightPlane {
                            values: vec![1.0],
                            mask: None,
                        },
                    )
                })
                .collect();
            rule_set.kernels[0].spatial =
                KernelSpatialDefinition::Periodic(PeriodicKernelDefinition {
                    width: 1,
                    height: 1,
                    anchor_x: 0,
                    anchor_y: 0,
                    planes,
                });
        }
        next.rules.defaults.insert(id, rule_set_id);
        next.rules.sets.push(rule_set);
        next.rules
            .bindings
            .extend(next.basis_ids().into_iter().map(|basis| RuleBinding {
                basis,
                output: id,
                rule_set: rule_set_id,
            }));
        Some(KernelId(0))
    };
    Ok(ChannelAddition {
        spec: next,
        channel: id,
        selected_kernel,
    })
}

/// Remove a channel and everything that depended on it, returning the new draft
/// and the channel that should take the selection.
///
/// The replacement is the channel that slid into the removed position, which
/// keeps the selection where the user was looking instead of jumping to the
/// start of the strip.
pub fn remove_channel(
    draft: &ExperimentSpec,
    removed: ChannelId,
) -> Result<(ExperimentSpec, ChannelId), String> {
    if draft.channels.len() <= 1 {
        return Err("an experiment must retain at least one channel".into());
    }
    let removed_position = draft
        .channels
        .iter()
        .position(|channel| channel.id == removed)
        .ok_or_else(|| "unknown channel".to_string())?;

    let mut next = draft.clone();
    if !next.rules.is_empty()
        && next.rules.sets.iter().any(|rule| {
            rule.kernels
                .iter()
                .any(|kernel| kernel.source_channel == removed && rule.growth.target != removed)
        })
    {
        return Err("channel is still used as a kernel source; reroute those kernels first".into());
    }

    next.channels.retain(|channel| channel.id != removed);
    next.kernels
        .retain(|kernel| kernel.source != removed && kernel.target != removed);
    next.growth.retain(|growth| growth.target != removed);
    for growth in &mut next.growth {
        growth.kernel_inputs.retain(|id| {
            next.kernels
                .iter()
                .any(|kernel| kernel.id == *id && kernel.target == growth.target)
        });
    }
    if !next.rules.is_empty() {
        next.rules
            .bindings
            .retain(|binding| binding.output != removed);
        next.rules.defaults.remove(&removed);
        let referenced = next
            .rules
            .bindings
            .iter()
            .map(|binding| binding.rule_set)
            .chain(next.rules.defaults.values().copied())
            .collect::<BTreeSet<_>>();
        next.rules.sets.retain(|rule| referenced.contains(&rule.id));
    }

    let nearest = next
        .channels
        .get(removed_position)
        .or_else(|| next.channels.last())
        .expect("channel minimum was checked")
        .id;
    Ok((next, nearest))
}

/// Freeze or unfreeze a channel, keeping the rule library consistent.
///
/// A frozen channel must not keep bindings or a default rule set, and thawing
/// one has to give it a rule set again, so this cannot be a plain field write.
pub fn set_channel_frozen(
    draft: &ExperimentSpec,
    target: ChannelId,
    frozen: bool,
) -> Result<ExperimentSpec, String> {
    let mut next = draft.clone();
    let Some(channel) = next
        .channels
        .iter_mut()
        .find(|channel| channel.id == target)
    else {
        return Err("unknown channel".into());
    };
    channel.frozen = frozen;
    if !next.rules.is_empty() {
        if frozen {
            next.rules
                .bindings
                .retain(|binding| binding.output != target);
            next.rules.defaults.remove(&target);
        } else {
            let rule_set = if let Some(rule_set) = next.rules.defaults.get(&target).copied() {
                rule_set
            } else if let Some(rule_set) = next
                .rules
                .sets
                .iter()
                .find(|rule| rule.growth.target == target)
                .map(|rule| rule.id)
            {
                next.rules.defaults.insert(target, rule_set);
                rule_set
            } else {
                let id = RuleSetId(
                    next.rules
                        .sets
                        .iter()
                        .map(|rule| rule.id.0)
                        .max()
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or("rule-set id exhausted")?,
                );
                let mut rule = RuleSet::identity(id, target);
                if next.tiling.is_some() {
                    let planes = next
                        .basis_ids()
                        .into_iter()
                        .map(|basis| {
                            (
                                basis,
                                crate::sim::basis_kernel::BasisWeightPlane {
                                    values: vec![1.0],
                                    mask: None,
                                },
                            )
                        })
                        .collect();
                    rule.kernels[0].spatial = KernelSpatialDefinition::Periodic(
                        crate::sim::basis_kernel::PeriodicKernelDefinition {
                            width: 1,
                            height: 1,
                            anchor_x: 0,
                            anchor_y: 0,
                            planes,
                        },
                    );
                }
                next.rules.defaults.insert(target, id);
                next.rules.sets.push(rule);
                id
            };
            for basis in next.basis_ids() {
                if next.rules.binding(basis, target).is_none() {
                    next.rules.bindings.push(RuleBinding {
                        basis,
                        output: target,
                        rule_set,
                    });
                }
            }
        }
        next.rules
            .validate(&next.basis_ids(), &next.channels)
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| error.to_string())
                    .collect::<Vec<_>>()
                    .join("; ")
            })?;
    } else if frozen {
        next.kernels.retain(|kernel| kernel.target != target);
        next.growth.retain(|growth| growth.target != target);
    } else if !next.growth.iter().any(|growth| growth.target == target) {
        next.growth
            .push(crate::sim::experiment_model::GrowthSource {
                target,
                kernel_inputs: Vec::new(),
                parameters: Default::default(),
                source: "self".into(),
                mode: UpdateMode::DirectUpdate,
            });
    }
    Ok(next)
}

/// Set the update mode of the growth program behind one binding, detaching a
/// shared rule set first so unrelated bindings keep their program.
pub fn set_growth_mode(
    draft: &ExperimentSpec,
    binding: BindingKey,
    mode: UpdateMode,
) -> Result<ExperimentSpec, String> {
    edit_growth(draft, binding, |growth| {
        growth.mode = mode;
        Ok(())
    })
}

/// Replace the growth source text of one binding.
pub fn set_growth_source(
    draft: &ExperimentSpec,
    binding: BindingKey,
    source: &str,
) -> Result<ExperimentSpec, String> {
    edit_growth(draft, binding, |growth| {
        growth.source = source.to_string();
        Ok(())
    })
}

fn edit_growth(
    draft: &ExperimentSpec,
    binding: BindingKey,
    edit: impl FnOnce(&mut crate::sim::experiment_model::GrowthSource) -> Result<(), String>,
) -> Result<ExperimentSpec, String> {
    let mut next = draft.clone();
    if next.rules.binding(binding.basis, binding.output).is_some() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let growth = &mut next
            .rules
            .get_mut(rule_set)
            .ok_or_else(|| "selected rule-set is missing".to_string())?
            .growth;
        edit(growth)?;
    } else {
        let growth = next
            .growth
            .iter_mut()
            .find(|growth| growth.target == binding.output)
            .ok_or_else(|| "selected growth program is missing".to_string())?;
        edit(growth)?;
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::experiment_model::validate_structure;

    #[test]
    fn adding_a_channel_keeps_the_experiment_valid_and_names_it_by_ordinal() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let added = add_channel(&spec).unwrap();
        assert_eq!(added.spec.channels.len(), 2);
        assert_eq!(added.spec.channels[1].id, added.channel);
        assert_eq!(added.spec.channels[1].name, "channel_2");
        assert!(added.selected_kernel.is_some());
        validate_structure(&added.spec).unwrap();
    }

    #[test]
    fn removing_the_last_remaining_channel_is_refused() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        assert_eq!(
            remove_channel(&spec, ChannelId(0)).unwrap_err(),
            "an experiment must retain at least one channel"
        );
    }

    #[test]
    fn removing_a_channel_selects_the_one_that_takes_its_position() {
        let mut spec = ExperimentSpec::single_channel_lenia(8, 8);
        for _ in 0..2 {
            spec = add_channel(&spec).unwrap().spec;
        }
        let (next, nearest) = remove_channel(&spec, ChannelId(1)).unwrap();
        assert_eq!(next.channels.len(), 2);
        assert_eq!(nearest, ChannelId(2));
        validate_structure(&next).unwrap();
    }

    #[test]
    fn removing_the_trailing_channel_falls_back_to_the_new_last_channel() {
        let mut spec = ExperimentSpec::single_channel_lenia(8, 8);
        for _ in 0..2 {
            spec = add_channel(&spec).unwrap().spec;
        }
        let (_, nearest) = remove_channel(&spec, ChannelId(2)).unwrap();
        assert_eq!(nearest, ChannelId(1));
    }

    #[test]
    fn an_unknown_channel_cannot_be_removed() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let two = add_channel(&spec).unwrap().spec;
        assert_eq!(
            remove_channel(&two, ChannelId(42)).unwrap_err(),
            "unknown channel"
        );
    }

    #[test]
    fn growth_edits_reach_the_program_of_the_selected_binding() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8);
        let binding = BindingKey {
            basis: spec.basis_ids()[0],
            output: ChannelId(0),
        };
        let next = set_growth_source(&spec, binding, "self").unwrap();
        let growth = growth_of(&next, binding);
        assert_eq!(growth.source, "self");

        let next = set_growth_mode(&next, binding, UpdateMode::DirectUpdate).unwrap();
        assert_eq!(growth_of(&next, binding).mode, UpdateMode::DirectUpdate);
    }

    fn growth_of(
        spec: &ExperimentSpec,
        binding: BindingKey,
    ) -> crate::sim::experiment_model::GrowthSource {
        match spec.rules.binding(binding.basis, binding.output) {
            Some(entry) => spec.rules.get(entry.rule_set).unwrap().growth.clone(),
            None => spec
                .growth
                .iter()
                .find(|growth| growth.target == binding.output)
                .unwrap()
                .clone(),
        }
    }
}
