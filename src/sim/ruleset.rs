use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::sim::basis_kernel::PeriodicKernelDefinition;
use crate::sim::experiment_model::{ChannelId, ChannelSpec, GrowthSource, KernelId, UpdateMode};
use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization};
use crate::sim::tiling::BasisId;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleSetId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BindingKey {
    pub basis: BasisId,
    pub output: ChannelId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum KernelSpatialDefinition {
    Raster(KernelDefinition),
    Periodic(PeriodicKernelDefinition),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleKernel {
    pub id: KernelId,
    pub symbol: String,
    pub name: String,
    pub source_channel: ChannelId,
    pub spatial: KernelSpatialDefinition,
}

impl RuleKernel {
    pub fn identity(id: KernelId, symbol: impl Into<String>, source_channel: ChannelId) -> Self {
        let symbol = symbol.into();
        Self {
            id,
            name: symbol.clone(),
            symbol,
            source_channel,
            spatial: KernelSpatialDefinition::Raster(KernelDefinition {
                name: "identity".to_string(),
                width: 1,
                height: 1,
                anchor_x: 0,
                anchor_y: 0,
                mask: None,
                normalization: Normalization::None,
                parameters: BTreeMap::new(),
                values: KernelValues::Explicit(vec![1.0]),
            }),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleSet {
    pub id: RuleSetId,
    pub shared_name: Option<String>,
    pub kernels: Vec<RuleKernel>,
    pub growth: GrowthSource,
}

impl RuleSet {
    pub fn identity(id: RuleSetId, target: ChannelId) -> Self {
        let kernel = RuleKernel::identity(KernelId(0), "potential", target);
        Self {
            id,
            shared_name: None,
            kernels: vec![kernel],
            growth: GrowthSource {
                target,
                kernel_inputs: vec![KernelId(0)],
                parameters: BTreeMap::new(),
                source: "potential".to_string(),
                mode: UpdateMode::DirectUpdate,
            },
        }
    }

    pub fn validate(&self) -> Result<(), RuleSetError> {
        let mut ids = BTreeSet::new();
        let mut symbols = BTreeSet::new();
        for kernel in &self.kernels {
            if !ids.insert(kernel.id) {
                return Err(RuleSetError::DuplicateKernelId {
                    rule_set: self.id,
                    kernel: kernel.id,
                });
            }
            if !valid_symbol(&kernel.symbol) || !symbols.insert(kernel.symbol.as_str()) {
                return Err(RuleSetError::InvalidKernelSymbol {
                    rule_set: self.id,
                    symbol: kernel.symbol.clone(),
                });
            }
            match &kernel.spatial {
                KernelSpatialDefinition::Raster(definition) => {
                    definition
                        .build()
                        .map_err(|error| RuleSetError::InvalidKernel {
                            rule_set: self.id,
                            kernel: kernel.id,
                            reason: error.to_string(),
                        })?;
                }
                KernelSpatialDefinition::Periodic(definition) => {
                    definition
                        .validate()
                        .map_err(|error| RuleSetError::InvalidKernel {
                            rule_set: self.id,
                            kernel: kernel.id,
                            reason: error.to_string(),
                        })?;
                }
            };
        }
        let expected = self
            .kernels
            .iter()
            .map(|kernel| kernel.id)
            .collect::<Vec<_>>();
        if self.growth.kernel_inputs != expected {
            return Err(RuleSetError::GrowthKernelMismatch {
                rule_set: self.id,
                expected,
                actual: self.growth.kernel_inputs.clone(),
            });
        }
        if self.growth.source.trim().is_empty() {
            return Err(RuleSetError::EmptyGrowthSource { rule_set: self.id });
        }
        if let Some(name) = &self.shared_name
            && name.trim().is_empty()
        {
            return Err(RuleSetError::InvalidSharedName { rule_set: self.id });
        }
        if let Some(parameter) = self.growth.parameters.iter().find_map(|(name, value)| {
            (name.trim().is_empty() || name == "self" || !value.is_finite()).then_some(name.clone())
        }) {
            return Err(RuleSetError::InvalidGrowthParameter {
                rule_set: self.id,
                parameter,
            });
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RuleBinding {
    pub basis: BasisId,
    pub output: ChannelId,
    pub rule_set: RuleSetId,
}

impl RuleBinding {
    pub fn key(&self) -> BindingKey {
        BindingKey {
            basis: self.basis,
            output: self.output,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleLibrary {
    pub defaults: BTreeMap<ChannelId, RuleSetId>,
    pub sets: Vec<RuleSet>,
    pub bindings: Vec<RuleBinding>,
}

impl RuleLibrary {
    pub fn is_empty(&self) -> bool {
        self.defaults.is_empty() && self.sets.is_empty() && self.bindings.is_empty()
    }

    pub fn get(&self, id: RuleSetId) -> Option<&RuleSet> {
        self.sets.iter().find(|rule| rule.id == id)
    }

    pub fn get_mut(&mut self, id: RuleSetId) -> Option<&mut RuleSet> {
        self.sets.iter_mut().find(|rule| rule.id == id)
    }

    pub fn binding(&self, basis: BasisId, output: ChannelId) -> Option<&RuleBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.basis == basis && binding.output == output)
    }

    pub fn detach(&mut self, binding: BindingKey) -> Result<RuleSetId, RuleSetError> {
        let current = self
            .binding(binding.basis, binding.output)
            .ok_or(RuleSetError::MissingBinding(binding))?
            .rule_set;
        let is_shared = self
            .bindings
            .iter()
            .filter(|entry| entry.rule_set == current)
            .count()
            > 1
            || self.defaults.values().any(|default| *default == current);
        if !is_shared {
            return Ok(current);
        }
        let mut detached = self
            .get(current)
            .cloned()
            .ok_or(RuleSetError::MissingRuleSet(current))?;
        let next = self
            .sets
            .iter()
            .map(|rule| rule.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(RuleSetError::RuleSetIdExhausted)?;
        detached.id = RuleSetId(next);
        detached.shared_name = None;
        self.sets.push(detached);
        self.binding_mut(binding)?.rule_set = RuleSetId(next);
        Ok(RuleSetId(next))
    }

    pub fn reset_to_default(&mut self, binding: BindingKey) -> Result<(), RuleSetError> {
        let default = *self
            .defaults
            .get(&binding.output)
            .ok_or(RuleSetError::MissingDefault(binding.output))?;
        self.binding_mut(binding)?.rule_set = default;
        Ok(())
    }

    pub fn edit_default(
        &mut self,
        channel: ChannelId,
        edit: impl FnOnce(&mut RuleSet),
    ) -> Result<(), RuleSetError> {
        let default = *self
            .defaults
            .get(&channel)
            .ok_or(RuleSetError::MissingDefault(channel))?;
        let mut replacement = self
            .get(default)
            .cloned()
            .ok_or(RuleSetError::MissingRuleSet(default))?;
        edit(&mut replacement);
        replacement.validate()?;
        *self
            .get_mut(default)
            .ok_or(RuleSetError::MissingRuleSet(default))? = replacement;
        Ok(())
    }

    fn binding_mut(&mut self, key: BindingKey) -> Result<&mut RuleBinding, RuleSetError> {
        self.bindings
            .iter_mut()
            .find(|binding| binding.key() == key)
            .ok_or(RuleSetError::MissingBinding(key))
    }

    pub fn validate(
        &self,
        basis_ids: &[BasisId],
        channels: &[ChannelSpec],
    ) -> Result<(), Vec<RuleSetError>> {
        let mut errors = Vec::new();
        let mut set_ids = BTreeSet::new();
        for rule in &self.sets {
            if !set_ids.insert(rule.id) {
                errors.push(RuleSetError::DuplicateRuleSetId(rule.id));
            }
            if let Err(error) = rule.validate() {
                errors.push(error);
            }
        }
        let channel_ids = channels
            .iter()
            .map(|channel| channel.id)
            .collect::<BTreeSet<_>>();
        let basis = basis_ids.iter().copied().collect::<BTreeSet<_>>();
        for rule in &self.sets {
            if !channel_ids.contains(&rule.growth.target) {
                errors.push(RuleSetError::MissingOutputChannel {
                    rule_set: rule.id,
                    channel: rule.growth.target,
                });
            }
            for kernel in &rule.kernels {
                if !channel_ids.contains(&kernel.source_channel) {
                    errors.push(RuleSetError::MissingSourceChannel {
                        rule_set: rule.id,
                        kernel: kernel.id,
                        channel: kernel.source_channel,
                    });
                }
                if let KernelSpatialDefinition::Periodic(definition) = &kernel.spatial {
                    for source_basis in definition.planes.keys() {
                        if !basis.contains(source_basis) {
                            errors.push(RuleSetError::MissingBasis(*source_basis));
                        }
                    }
                }
            }
        }

        let active = channels
            .iter()
            .filter(|channel| !channel.frozen)
            .map(|channel| channel.id)
            .collect::<BTreeSet<_>>();
        let mut binding_keys = BTreeSet::new();
        for binding in &self.bindings {
            if !binding_keys.insert(binding.key()) {
                errors.push(RuleSetError::DuplicateBinding(binding.key()));
            }
            if !basis.contains(&binding.basis) {
                errors.push(RuleSetError::MissingBasis(binding.basis));
            }
            if !active.contains(&binding.output) {
                errors.push(RuleSetError::InvalidBindingOutput(binding.output));
            }
            match self.get(binding.rule_set) {
                None => errors.push(RuleSetError::MissingRuleSet(binding.rule_set)),
                Some(rule) if rule.growth.target != binding.output => {
                    errors.push(RuleSetError::BindingTargetMismatch {
                        binding: binding.key(),
                        target: rule.growth.target,
                    });
                }
                Some(_) => {}
            }
        }
        for basis in basis_ids {
            for output in &active {
                let key = BindingKey {
                    basis: *basis,
                    output: *output,
                };
                if !binding_keys.contains(&key) {
                    errors.push(RuleSetError::MissingBinding(key));
                }
            }
        }
        for channel in channels {
            match (channel.frozen, self.defaults.get(&channel.id)) {
                (false, None) => errors.push(RuleSetError::MissingDefault(channel.id)),
                (true, Some(_)) => errors.push(RuleSetError::FrozenChannelDefault(channel.id)),
                (_, Some(rule_set)) if self.get(*rule_set).is_none() => {
                    errors.push(RuleSetError::MissingRuleSet(*rule_set))
                }
                (_, Some(rule_set))
                    if self
                        .get(*rule_set)
                        .is_some_and(|rule| rule.growth.target != channel.id) =>
                {
                    errors.push(RuleSetError::DefaultTargetMismatch {
                        channel: channel.id,
                        rule_set: *rule_set,
                    })
                }
                _ => {}
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }
}

fn valid_symbol(symbol: &str) -> bool {
    let mut characters = symbol.chars();
    let Some(first) = characters.next() else {
        return false;
    };
    (first == '_' || first.is_ascii_alphabetic())
        && characters.all(|character| character == '_' || character.is_ascii_alphanumeric())
        && symbol != "self"
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum RuleSetError {
    #[error("rule-set ID {0:?} is duplicated")]
    DuplicateRuleSetId(RuleSetId),
    #[error("rule-set {rule_set:?} has duplicate kernel ID {kernel:?}")]
    DuplicateKernelId {
        rule_set: RuleSetId,
        kernel: KernelId,
    },
    #[error("rule-set {rule_set:?} has invalid or duplicate kernel symbol `{symbol}`")]
    InvalidKernelSymbol { rule_set: RuleSetId, symbol: String },
    #[error("rule-set {rule_set:?} kernel {kernel:?} is invalid: {reason}")]
    InvalidKernel {
        rule_set: RuleSetId,
        kernel: KernelId,
        reason: String,
    },
    #[error("rule-set {rule_set:?} growth inputs {actual:?}; expected {expected:?}")]
    GrowthKernelMismatch {
        rule_set: RuleSetId,
        expected: Vec<KernelId>,
        actual: Vec<KernelId>,
    },
    #[error("rule-set {rule_set:?} has empty growth source")]
    EmptyGrowthSource { rule_set: RuleSetId },
    #[error("rule-set {rule_set:?} has an empty shared name")]
    InvalidSharedName { rule_set: RuleSetId },
    #[error("rule-set {rule_set:?} has invalid growth parameter `{parameter}`")]
    InvalidGrowthParameter {
        rule_set: RuleSetId,
        parameter: String,
    },
    #[error("rule-set {rule_set:?} targets missing channel {channel:?}")]
    MissingOutputChannel {
        rule_set: RuleSetId,
        channel: ChannelId,
    },
    #[error("rule-set {rule_set:?} kernel {kernel:?} reads missing channel {channel:?}")]
    MissingSourceChannel {
        rule_set: RuleSetId,
        kernel: KernelId,
        channel: ChannelId,
    },
    #[error("binding {0:?} is duplicated")]
    DuplicateBinding(BindingKey),
    #[error("a rule is bound to basis {}, which the tiling no longer has", .0.0)]
    MissingBasis(BasisId),
    #[error("binding targets missing or frozen channel {0:?}")]
    InvalidBindingOutput(ChannelId),
    #[error("binding references missing rule-set {0:?}")]
    MissingRuleSet(RuleSetId),
    #[error("binding {binding:?} points to a rule targeting {target:?}")]
    BindingTargetMismatch {
        binding: BindingKey,
        target: ChannelId,
    },
    #[error("active basis/channel binding {0:?} is missing")]
    MissingBinding(BindingKey),
    #[error("active channel {0:?} has no default rule-set")]
    MissingDefault(ChannelId),
    #[error("frozen channel {0:?} must not have a default rule-set")]
    FrozenChannelDefault(ChannelId),
    #[error("channel {channel:?} default {rule_set:?} targets a different channel")]
    DefaultTargetMismatch {
        channel: ChannelId,
        rule_set: RuleSetId,
    },
    #[error("no further rule-set IDs are available")]
    RuleSetIdExhausted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::basis_kernel::PeriodicKernelDefinition;
    use crate::sim::experiment_model::{ChannelDisplay, ChannelId, ChannelSpec, KernelId};
    use crate::sim::tiling::BasisId;

    fn channel(id: u32, frozen: bool) -> ChannelSpec {
        ChannelSpec {
            id: ChannelId(id),
            name: format!("channel-{id}"),
            frozen,
            initial: vec![0.0],
            boundary_constant: 0.0,
            display: ChannelDisplay::default(),
        }
    }

    fn valid_library() -> RuleLibrary {
        RuleLibrary {
            defaults: BTreeMap::from([(ChannelId(0), RuleSetId(7))]),
            sets: vec![RuleSet::identity(RuleSetId(7), ChannelId(0))],
            bindings: vec![
                RuleBinding {
                    basis: BasisId(10),
                    output: ChannelId(0),
                    rule_set: RuleSetId(7),
                },
                RuleBinding {
                    basis: BasisId(20),
                    output: ChannelId(0),
                    rule_set: RuleSetId(7),
                },
            ],
        }
    }

    #[test]
    fn growth_arity_is_kernel_arity() {
        let mut rule = RuleSet::identity(RuleSetId(1), ChannelId(0));
        rule.kernels
            .push(RuleKernel::identity(KernelId(2), "outer", ChannelId(0)));

        assert!(matches!(
            rule.validate(),
            Err(RuleSetError::GrowthKernelMismatch { .. })
        ));
    }

    #[test]
    fn rule_kernel_symbols_are_stable_and_unique() {
        let mut rule = RuleSet::identity(RuleSetId(1), ChannelId(0));
        rule.kernels
            .push(RuleKernel::identity(KernelId(2), "potential", ChannelId(0)));
        rule.growth.kernel_inputs.push(KernelId(2));

        assert!(matches!(
            rule.validate(),
            Err(RuleSetError::InvalidKernelSymbol { .. })
        ));
    }

    #[test]
    fn every_active_basis_channel_pair_has_exactly_one_binding() {
        let mut library = valid_library();
        library.bindings.pop();
        assert!(
            library
                .validate(&[BasisId(10), BasisId(20)], &[channel(0, false)])
                .unwrap_err()
                .contains(&RuleSetError::MissingBinding(BindingKey {
                    basis: BasisId(20),
                    output: ChannelId(0),
                }))
        );

        library.bindings.push(RuleBinding {
            basis: BasisId(10),
            output: ChannelId(0),
            rule_set: RuleSetId(7),
        });
        assert!(
            library
                .validate(&[BasisId(10), BasisId(20)], &[channel(0, false)])
                .unwrap_err()
                .iter()
                .any(|error| matches!(error, RuleSetError::DuplicateBinding(_)))
        );
    }

    #[test]
    fn frozen_channels_have_neither_defaults_nor_bindings() {
        let mut library = valid_library();
        library.defaults.insert(ChannelId(1), RuleSetId(7));
        library.bindings.push(RuleBinding {
            basis: BasisId(10),
            output: ChannelId(1),
            rule_set: RuleSetId(7),
        });
        let errors = library
            .validate(
                &[BasisId(10), BasisId(20)],
                &[channel(0, false), channel(1, true)],
            )
            .unwrap_err();
        assert!(errors.contains(&RuleSetError::FrozenChannelDefault(ChannelId(1))));
        assert!(errors.contains(&RuleSetError::InvalidBindingOutput(ChannelId(1))));
    }

    #[test]
    fn periodic_kernel_rejects_unknown_source_basis() {
        let mut library = valid_library();
        library.sets[0].kernels[0].spatial =
            KernelSpatialDefinition::Periodic(PeriodicKernelDefinition::identity(BasisId(999)));
        assert!(
            library
                .validate(&[BasisId(10), BasisId(20)], &[channel(0, false)])
                .unwrap_err()
                .contains(&RuleSetError::MissingBasis(BasisId(999)))
        );
    }

    #[test]
    fn kernel_symbol_must_be_a_language_identifier() {
        let mut rule = RuleSet::identity(RuleSetId(1), ChannelId(0));
        rule.kernels[0].symbol = "not a symbol".to_string();
        assert!(matches!(
            rule.validate(),
            Err(RuleSetError::InvalidKernelSymbol { .. })
        ));
    }

    #[test]
    fn detach_and_reset_preserve_shared_default_semantics() {
        let mut library = valid_library();
        let first = BindingKey {
            basis: BasisId(10),
            output: ChannelId(0),
        };
        let detached = library.detach(first).unwrap();
        assert_ne!(detached, RuleSetId(7));
        library.get_mut(detached).unwrap().shared_name = Some("local first".into());
        assert_eq!(
            library.binding(BasisId(20), ChannelId(0)).unwrap().rule_set,
            RuleSetId(7)
        );

        library.reset_to_default(first).unwrap();
        assert_eq!(
            library.binding(BasisId(10), ChannelId(0)).unwrap().rule_set,
            RuleSetId(7)
        );
        assert_eq!(
            library.get(detached).unwrap().shared_name.as_deref(),
            Some("local first")
        );
    }

    #[test]
    fn editing_default_updates_shared_bindings_transactionally() {
        let mut library = valid_library();
        library
            .edit_default(ChannelId(0), |rule| {
                rule.shared_name = Some("all polygons".into())
            })
            .unwrap();
        assert_eq!(
            library
                .get(library.binding(BasisId(10), ChannelId(0)).unwrap().rule_set)
                .unwrap()
                .shared_name
                .as_deref(),
            Some("all polygons")
        );

        let before = library.clone();
        assert!(
            library
                .edit_default(ChannelId(0), |rule| rule.kernels[0].symbol =
                    "bad symbol".into())
                .is_err()
        );
        assert_eq!(library, before);
    }
}
