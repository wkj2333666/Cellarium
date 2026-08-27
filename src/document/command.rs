use crate::sim::experiment_model::{ChannelId, DisplayColor, ExperimentSpec, KernelId};
use crate::sim::ruleset::{BindingKey, KernelSpatialDefinition, RuleKernel, RuleSet, RuleSetId};
use crate::sim::tiling::BasisId;

#[derive(Clone, Debug, PartialEq)]
pub enum DraftCommand {
    SetChannelValue {
        channel: ChannelId,
        tile: usize,
        value: f32,
    },
    SetChannelValues {
        channel: ChannelId,
        values: Vec<(usize, f32)>,
    },
    RenameChannel {
        channel: ChannelId,
        name: String,
    },
    SetChannelColor {
        channel: ChannelId,
        color: DisplayColor,
    },
    SetChannelVisible {
        channel: ChannelId,
        visible: bool,
    },
    SetChannelFrozen {
        channel: ChannelId,
        frozen: bool,
    },
    DetachRuleSet {
        binding: BindingKey,
    },
    RestoreDetachedRuleSet {
        binding: BindingKey,
        previous: RuleSetId,
        detached: RuleSetId,
    },
    ResetRuleSetToDefault {
        binding: BindingKey,
    },
    SetRuleBinding {
        binding: BindingKey,
        rule_set: RuleSetId,
    },
    ReplaceRuleSet(Box<RuleSet>),
    SetPeriodicKernelWeight {
        rule_set: RuleSetId,
        kernel: KernelId,
        offset: [i16; 2],
        source_basis: BasisId,
        value: f32,
    },
    AddKernel {
        rule_set: RuleSetId,
        kernel: RuleKernel,
    },
    RemoveKernel {
        rule_set: RuleSetId,
        kernel: KernelId,
    },
    InsertKernel {
        rule_set: RuleSetId,
        index: usize,
        kernel: RuleKernel,
    },
    ReplaceDraft(Box<ExperimentSpec>),
}

impl DraftCommand {
    pub fn apply(&self, draft: &mut ExperimentSpec) -> Result<Self, String> {
        match self {
            Self::SetChannelValue {
                channel,
                tile,
                value,
            } => {
                if !value.is_finite() || !(0.0..=1.0).contains(value) {
                    return Err("channel value must be finite and within 0..=1".into());
                }
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = *target
                    .initial
                    .get(*tile)
                    .ok_or_else(|| "tile index is outside the channel".to_string())?;
                target.initial[*tile] = *value;
                Ok(Self::SetChannelValue {
                    channel: *channel,
                    tile: *tile,
                    value: previous,
                })
            }
            Self::SetChannelValues { channel, values } => {
                if values
                    .iter()
                    .any(|(_, value)| !value.is_finite() || !(0.0..=1.0).contains(value))
                {
                    return Err("channel values must be finite and within 0..=1".into());
                }
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                if values.iter().any(|(tile, _)| *tile >= target.initial.len()) {
                    return Err("tile index is outside the channel".into());
                }
                let previous = values
                    .iter()
                    .map(|(tile, _)| (*tile, target.initial[*tile]))
                    .collect::<Vec<_>>();
                for (tile, value) in values {
                    target.initial[*tile] = *value;
                }
                Ok(Self::SetChannelValues {
                    channel: *channel,
                    values: previous,
                })
            }
            Self::RenameChannel { channel, name } => {
                let trimmed = name.trim();
                if trimmed.is_empty()
                    || draft
                        .channels
                        .iter()
                        .any(|entry| entry.id != *channel && entry.name == trimmed)
                {
                    return Err("channel name must be non-empty and unique".into());
                }
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.name, trimmed.to_string());
                Ok(Self::RenameChannel {
                    channel: *channel,
                    name: previous,
                })
            }
            Self::SetChannelColor { channel, color } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.display.color, color.clone());
                Ok(Self::SetChannelColor {
                    channel: *channel,
                    color: previous,
                })
            }
            Self::SetChannelVisible { channel, visible } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.display.visible, *visible);
                Ok(Self::SetChannelVisible {
                    channel: *channel,
                    visible: previous,
                })
            }
            Self::SetChannelFrozen { channel, frozen } => {
                let target = draft
                    .channels
                    .iter_mut()
                    .find(|entry| entry.id == *channel)
                    .ok_or_else(|| "unknown channel".to_string())?;
                let previous = std::mem::replace(&mut target.frozen, *frozen);
                Ok(Self::SetChannelFrozen {
                    channel: *channel,
                    frozen: previous,
                })
            }
            Self::DetachRuleSet { binding } => {
                let previous = draft
                    .rules
                    .binding(binding.basis, binding.output)
                    .ok_or_else(|| format!("missing rule binding {binding:?}"))?
                    .rule_set;
                let detached = draft
                    .rules
                    .detach(*binding)
                    .map_err(|error| error.to_string())?;
                Ok(Self::RestoreDetachedRuleSet {
                    binding: *binding,
                    previous,
                    detached,
                })
            }
            Self::RestoreDetachedRuleSet {
                binding,
                previous,
                detached,
            } => {
                let current = draft
                    .rules
                    .binding(binding.basis, binding.output)
                    .ok_or_else(|| format!("missing rule binding {binding:?}"))?
                    .rule_set;
                if current != *detached {
                    return Err("detached rule-set changed before undo".into());
                }
                Self::SetRuleBinding {
                    binding: *binding,
                    rule_set: *previous,
                }
                .apply(draft)?;
                if !draft
                    .rules
                    .bindings
                    .iter()
                    .any(|entry| entry.rule_set == *detached)
                    && !draft.rules.defaults.values().any(|id| *id == *detached)
                {
                    draft.rules.sets.retain(|rule| rule.id != *detached);
                }
                Ok(Self::DetachRuleSet { binding: *binding })
            }
            Self::ResetRuleSetToDefault { binding } => {
                let previous = draft
                    .rules
                    .binding(binding.basis, binding.output)
                    .ok_or_else(|| format!("missing rule binding {binding:?}"))?
                    .rule_set;
                draft
                    .rules
                    .reset_to_default(*binding)
                    .map_err(|error| error.to_string())?;
                Ok(Self::SetRuleBinding {
                    binding: *binding,
                    rule_set: previous,
                })
            }
            Self::SetRuleBinding { binding, rule_set } => {
                if draft.rules.get(*rule_set).is_none() {
                    return Err(format!("missing rule-set {rule_set:?}"));
                }
                let target = draft
                    .rules
                    .bindings
                    .iter_mut()
                    .find(|entry| entry.key() == *binding)
                    .ok_or_else(|| format!("missing rule binding {binding:?}"))?;
                let previous = std::mem::replace(&mut target.rule_set, *rule_set);
                Ok(Self::SetRuleBinding {
                    binding: *binding,
                    rule_set: previous,
                })
            }
            Self::ReplaceRuleSet(replacement) => {
                replacement.validate().map_err(|error| error.to_string())?;
                let target = draft
                    .rules
                    .sets
                    .iter_mut()
                    .find(|rule| rule.id == replacement.id)
                    .ok_or_else(|| format!("missing rule-set {:?}", replacement.id))?;
                let previous = std::mem::replace(target, replacement.as_ref().clone());
                Ok(Self::ReplaceRuleSet(Box::new(previous)))
            }
            Self::SetPeriodicKernelWeight {
                rule_set,
                kernel,
                offset,
                source_basis,
                value,
            } => {
                let target = draft
                    .rules
                    .get_mut(*rule_set)
                    .and_then(|rule| rule.kernels.iter_mut().find(|entry| entry.id == *kernel))
                    .ok_or_else(|| "selected rule kernel is missing".to_string())?;
                let KernelSpatialDefinition::Periodic(definition) = &mut target.spatial else {
                    return Err("selected kernel is not periodic".into());
                };
                let previous = definition
                    .weight(*offset, *source_basis)
                    .ok_or_else(|| "selected periodic weight is unavailable".to_string())?;
                definition
                    .set_weight(*offset, *source_basis, *value)
                    .map_err(|error| error.to_string())?;
                Ok(Self::SetPeriodicKernelWeight {
                    rule_set: *rule_set,
                    kernel: *kernel,
                    offset: *offset,
                    source_basis: *source_basis,
                    value: previous,
                })
            }
            Self::AddKernel { rule_set, kernel } => {
                let rule = draft
                    .rules
                    .get(*rule_set)
                    .cloned()
                    .ok_or_else(|| format!("missing rule-set {rule_set:?}"))?;
                let mut replacement = rule;
                replacement.kernels.push(kernel.clone());
                replacement.growth.kernel_inputs.push(kernel.id);
                replacement.validate().map_err(|error| error.to_string())?;
                Self::ReplaceRuleSet(Box::new(replacement)).apply(draft)?;
                Ok(Self::RemoveKernel {
                    rule_set: *rule_set,
                    kernel: kernel.id,
                })
            }
            Self::RemoveKernel { rule_set, kernel } => {
                let rule = draft
                    .rules
                    .get(*rule_set)
                    .cloned()
                    .ok_or_else(|| format!("missing rule-set {rule_set:?}"))?;
                let mut replacement = rule;
                let index = replacement
                    .kernels
                    .iter()
                    .position(|entry| entry.id == *kernel)
                    .ok_or_else(|| format!("missing kernel {kernel:?}"))?;
                let removed = replacement.kernels.remove(index);
                replacement.growth.kernel_inputs.remove(index);
                replacement.validate().map_err(|error| error.to_string())?;
                Self::ReplaceRuleSet(Box::new(replacement)).apply(draft)?;
                Ok(Self::InsertKernel {
                    rule_set: *rule_set,
                    index,
                    kernel: removed,
                })
            }
            Self::InsertKernel {
                rule_set,
                index,
                kernel,
            } => {
                let rule = draft
                    .rules
                    .get(*rule_set)
                    .cloned()
                    .ok_or_else(|| format!("missing rule-set {rule_set:?}"))?;
                let mut replacement = rule;
                if *index > replacement.kernels.len() {
                    return Err("kernel insertion index is invalid".into());
                }
                replacement.kernels.insert(*index, kernel.clone());
                replacement.growth.kernel_inputs.insert(*index, kernel.id);
                replacement.validate().map_err(|error| error.to_string())?;
                Self::ReplaceRuleSet(Box::new(replacement)).apply(draft)?;
                Ok(Self::RemoveKernel {
                    rule_set: *rule_set,
                    kernel: kernel.id,
                })
            }
            Self::ReplaceDraft(replacement) => {
                let previous = std::mem::replace(draft, replacement.as_ref().clone());
                Ok(Self::ReplaceDraft(Box::new(previous)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::History;
    use crate::sim::experiment_model::ExperimentSpec;
    use crate::sim::ruleset::RuleKernel;
    use crate::sim::tiling::BasisId;

    #[test]
    fn add_and_remove_kernel_update_growth_arity_with_exact_undo() {
        let mut draft = ExperimentSpec::single_channel_lenia(2, 2)
            .normalize_rules()
            .unwrap();
        let rule_set = draft.rules.sets[0].id;
        let original = draft.clone();
        let kernel = RuleKernel::identity(KernelId(9), "outer", ChannelId(0));
        let mut history = History::default();

        history
            .execute(&mut draft, DraftCommand::AddKernel { rule_set, kernel })
            .unwrap();
        assert_eq!(draft.rules.get(rule_set).unwrap().kernels.len(), 2);
        assert_eq!(
            draft.rules.get(rule_set).unwrap().growth.kernel_inputs,
            vec![KernelId(0), KernelId(9)]
        );
        history.undo(&mut draft).unwrap();
        assert_eq!(draft, original);
        history.redo(&mut draft).unwrap();
        assert_eq!(draft.rules.get(rule_set).unwrap().kernels.len(), 2);
    }

    #[test]
    fn detach_and_reset_binding_roundtrip_through_history() {
        let mut draft = ExperimentSpec::single_channel_lenia(2, 2)
            .normalize_rules()
            .unwrap();
        let binding = BindingKey {
            basis: BasisId(0),
            output: ChannelId(0),
        };
        let default = draft
            .rules
            .binding(BasisId(0), ChannelId(0))
            .unwrap()
            .rule_set;
        let mut history = History::default();
        history
            .execute(&mut draft, DraftCommand::DetachRuleSet { binding })
            .unwrap();
        let local = draft
            .rules
            .binding(BasisId(0), ChannelId(0))
            .unwrap()
            .rule_set;
        assert_ne!(local, default);
        history
            .execute(&mut draft, DraftCommand::ResetRuleSetToDefault { binding })
            .unwrap();
        assert_eq!(
            draft
                .rules
                .binding(BasisId(0), ChannelId(0))
                .unwrap()
                .rule_set,
            default
        );
        history.undo(&mut draft).unwrap();
        assert_eq!(
            draft
                .rules
                .binding(BasisId(0), ChannelId(0))
                .unwrap()
                .rule_set,
            local
        );
        history.undo(&mut draft).unwrap();
        assert_eq!(
            draft
                .rules
                .binding(BasisId(0), ChannelId(0))
                .unwrap()
                .rule_set,
            default
        );
    }
}
