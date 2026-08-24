use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::sim::kernel::{KernelDefinition, KernelValues, Normalization, ring_definition};
use crate::sim::ruleset::{
    KernelSpatialDefinition, RuleBinding, RuleKernel, RuleLibrary, RuleSet, RuleSetId,
};
use crate::sim::tiling::{
    BasisId, PeriodicTilingDraft,
    polygon::{prototype_vertices, validate_polygon},
};
use crate::sim::topology::BoundarySpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct KernelId(pub u32);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelSpec {
    pub id: ChannelId,
    pub name: String,
    pub frozen: bool,
    pub initial: Vec<f32>,
    pub boundary_constant: f32,
    pub display: ChannelDisplay,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChannelDisplay {
    pub color: DisplayColor,
    pub visible: bool,
    pub opacity: f32,
}

impl Default for ChannelDisplay {
    fn default() -> Self {
        Self {
            color: DisplayColor::Auto,
            visible: true,
            opacity: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayColor {
    Auto,
    Custom(RgbColor),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RgbColor {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelSlot {
    pub id: KernelId,
    pub symbol: String,
    pub name: String,
    pub source: ChannelId,
    pub target: ChannelId,
    pub definition: KernelDefinition,
}

impl KernelSlot {
    pub fn identity(
        id: KernelId,
        symbol: impl Into<String>,
        source: ChannelId,
        target: ChannelId,
    ) -> Self {
        let symbol = symbol.into();
        Self {
            id,
            name: symbol.clone(),
            symbol,
            source,
            target,
            definition: KernelDefinition {
                name: "identity".to_string(),
                width: 1,
                height: 1,
                anchor_x: 0,
                anchor_y: 0,
                mask: None,
                normalization: Normalization::None,
                parameters: BTreeMap::new(),
                values: KernelValues::Explicit(vec![1.0]),
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateMode {
    GrowthRate,
    DirectUpdate,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GrowthSource {
    pub target: ChannelId,
    pub kernel_inputs: Vec<KernelId>,
    pub parameters: BTreeMap<String, f32>,
    pub source: String,
    pub mode: UpdateMode,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum GeometrySpec {
    RasterGrid(GridGeometry),
}

impl GeometrySpec {
    pub fn tile_count(&self) -> Option<usize> {
        match self {
            Self::RasterGrid(grid) => (grid.width as usize).checked_mul(grid.height as usize),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridGeometry {
    pub width: u32,
    pub height: u32,
    pub boundary: BoundarySpec,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExperimentSpec {
    pub name: String,
    pub geometry: GeometrySpec,
    pub channels: Vec<ChannelSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kernels: Vec<KernelSlot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub growth: Vec<GrowthSource>,
    /// Normalized basis-aware rules. Empty means a legacy global rule model
    /// that has not yet crossed the one-way normalization boundary.
    #[serde(default, skip_serializing_if = "RuleLibrary::is_empty")]
    pub rules: RuleLibrary,
    pub simulation_dt: f32,
    pub seed: u64,
    /// Optional polygonal tiling metadata. Raster execution remains the
    /// compatibility path; the geometry is validated and persisted atomically
    /// so the Workbench can edit it without losing the draft.
    #[serde(default)]
    pub tiling: Option<PeriodicTilingDraft>,
}

impl ExperimentSpec {
    pub fn single_channel_lenia(width: u32, height: u32) -> Self {
        let channel = ChannelId(0);
        let potential = KernelId(0);
        let tile_count = (width as usize).saturating_mul(height as usize);
        Self {
            name: "Lenia/Orbium".to_string(),
            geometry: GeometrySpec::RasterGrid(GridGeometry {
                width,
                height,
                boundary: BoundarySpec::Periodic,
            }),
            channels: vec![ChannelSpec {
                id: channel,
                name: "state".to_string(),
                frozen: false,
                initial: vec![0.0; tile_count],
                boundary_constant: 0.0,
                display: ChannelDisplay::default(),
            }],
            kernels: vec![KernelSlot {
                id: potential,
                symbol: "potential".to_string(),
                name: "ring".to_string(),
                source: channel,
                target: channel,
                definition: ring_definition(13, 0.5, 0.5),
            }],
            growth: vec![GrowthSource {
                target: channel,
                kernel_inputs: vec![potential],
                parameters: BTreeMap::from([
                    ("mu".to_string(), 0.135),
                    ("sigma".to_string(), 0.015),
                ]),
                source: "2 * exp(-((potential - mu) / sigma) ^ 2) - 1".to_string(),
                mode: UpdateMode::GrowthRate,
            }],
            rules: RuleLibrary::default(),
            simulation_dt: 0.1,
            seed: 0,
            tiling: None,
        }
    }

    pub fn add_channel(&mut self, name: impl Into<String>, frozen: bool) -> ChannelId {
        let id = ChannelId(
            self.channels
                .iter()
                .map(|channel| channel.id.0.saturating_add(1))
                .max()
                .unwrap_or(0),
        );
        self.channels.push(ChannelSpec {
            id,
            name: name.into(),
            frozen,
            initial: vec![0.0; self.geometry.tile_count().unwrap_or(0)],
            boundary_constant: 0.0,
            display: ChannelDisplay::default(),
        });
        if !frozen {
            self.growth.push(GrowthSource {
                target: id,
                kernel_inputs: Vec::new(),
                parameters: BTreeMap::new(),
                source: "self".to_string(),
                mode: UpdateMode::DirectUpdate,
            });
        }
        id
    }

    pub fn basis_ids(&self) -> Vec<BasisId> {
        let mut ids = self.tiling.as_ref().map_or_else(
            || vec![BasisId(0)],
            |tiling| {
                tiling
                    .instances
                    .iter()
                    .map(|instance| instance.id)
                    .collect()
            },
        );
        ids.sort_unstable();
        ids.dedup();
        if ids.is_empty() {
            ids.push(BasisId(0));
        }
        ids
    }

    pub fn normalize_rules(mut self) -> Result<Self, Vec<ExperimentModelError>> {
        let has_legacy = !self.kernels.is_empty() || !self.growth.is_empty();
        let has_normalized = !self.rules.is_empty();
        if has_legacy && has_normalized {
            return Err(vec![ExperimentModelError::AmbiguousRuleRepresentations]);
        }
        if !has_normalized {
            validate_structure(&self)?;
            let basis_ids = self.basis_ids();
            let active_channels = self
                .channels
                .iter()
                .filter(|channel| !channel.frozen)
                .map(|channel| channel.id)
                .collect::<Vec<_>>();
            for (index, output) in active_channels.iter().copied().enumerate() {
                let Some(growth) = self
                    .growth
                    .iter()
                    .find(|growth| growth.target == output)
                    .cloned()
                else {
                    return Err(vec![ExperimentModelError::MissingGrowthProgram(output)]);
                };
                let id = RuleSetId(u32::try_from(index).map_err(|_| {
                    vec![ExperimentModelError::InvalidRules(
                        "too many active channels".to_string(),
                    )]
                })?);
                let mut kernels = self
                    .kernels
                    .iter()
                    .filter(|kernel| kernel.target == output)
                    .map(|kernel| RuleKernel {
                        id: kernel.id,
                        symbol: kernel.symbol.clone(),
                        name: kernel.name.clone(),
                        source_channel: kernel.source,
                        spatial: KernelSpatialDefinition::Raster(kernel.definition.clone()),
                    })
                    .collect::<Vec<_>>();
                kernels.sort_by_key(|kernel| kernel.id);
                self.rules.defaults.insert(output, id);
                self.rules.sets.push(RuleSet {
                    id,
                    shared_name: None,
                    kernels,
                    growth,
                });
                self.rules
                    .bindings
                    .extend(basis_ids.iter().map(|basis| RuleBinding {
                        basis: *basis,
                        output,
                        rule_set: id,
                    }));
            }
            self.kernels.clear();
            self.growth.clear();
        }
        if let Err(errors) = self.rules.validate(&self.basis_ids(), &self.channels) {
            return Err(errors
                .into_iter()
                .map(|error| ExperimentModelError::InvalidRules(error.to_string()))
                .collect());
        }
        Ok(self)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ExperimentModelError {
    #[error("an experiment must contain at least one channel")]
    EmptyChannels,
    #[error("raster grid dimensions must be positive and representable")]
    InvalidGeometry,
    #[error("simulation dt must be finite and positive")]
    InvalidSimulationDt,
    #[error("channel ID {0:?} is duplicated")]
    DuplicateChannelId(ChannelId),
    #[error("channel name `{0}` is empty or duplicated")]
    InvalidChannelName(String),
    #[error("channel {channel:?} has {actual} cells; expected {expected}")]
    InvalidInitialLength {
        channel: ChannelId,
        expected: usize,
        actual: usize,
    },
    #[error("channel {0:?} contains a non-finite value")]
    NonFiniteChannel(ChannelId),
    #[error("channel {0:?} has a non-finite boundary constant")]
    NonFiniteBoundary(ChannelId),
    #[error("channel {0:?} has opacity outside 0..=1")]
    InvalidOpacity(ChannelId),
    #[error("kernel ID {0:?} is duplicated")]
    DuplicateKernelId(KernelId),
    #[error("kernel symbol `{0}` is empty, reserved, or duplicated")]
    InvalidKernelSymbol(String),
    #[error("kernel {kernel:?} refers to missing {role} channel {channel:?}")]
    MissingKernelChannel {
        kernel: KernelId,
        role: &'static str,
        channel: ChannelId,
    },
    #[error("kernel {kernel:?} targets frozen channel {target:?}")]
    FrozenKernelTarget { kernel: KernelId, target: ChannelId },
    #[error("kernel {kernel:?} is invalid: {reason}")]
    InvalidKernel { kernel: KernelId, reason: String },
    #[error("growth program target {0:?} does not exist")]
    MissingGrowthTarget(ChannelId),
    #[error("growth program target {0:?} is duplicated")]
    DuplicateGrowthTarget(ChannelId),
    #[error("frozen channel {0:?} must not have a growth program")]
    FrozenGrowthTarget(ChannelId),
    #[error("active channel {0:?} has no growth program")]
    MissingGrowthProgram(ChannelId),
    #[error("growth program for {target:?} has kernel inputs {actual:?}; expected {expected:?}")]
    GrowthKernelMismatch {
        target: ChannelId,
        expected: Vec<KernelId>,
        actual: Vec<KernelId>,
    },
    #[error("growth program for {target:?} has invalid parameter `{parameter}`")]
    InvalidGrowthParameter {
        target: ChannelId,
        parameter: String,
    },
    #[error("growth program for {0:?} is empty")]
    EmptyGrowthSource(ChannelId),
    #[error("tiling is invalid: {0}")]
    InvalidTiling(String),
    #[error("basis rules are invalid: {0}")]
    InvalidRules(String),
    #[error("legacy global rules and normalized basis rules cannot both be present")]
    AmbiguousRuleRepresentations,
}

pub fn validate_structure(spec: &ExperimentSpec) -> Result<(), Vec<ExperimentModelError>> {
    let mut errors = Vec::new();
    let tile_count = match &spec.geometry {
        GeometrySpec::RasterGrid(grid) => {
            if grid.width == 0 || grid.height == 0 {
                errors.push(ExperimentModelError::InvalidGeometry);
                None
            } else {
                if matches!(grid.boundary, BoundarySpec::Constant(value) if !value.is_finite()) {
                    errors.push(ExperimentModelError::InvalidGeometry);
                }
                (grid.width as usize).checked_mul(grid.height as usize)
            }
        }
    };
    if tile_count.is_none() && !errors.contains(&ExperimentModelError::InvalidGeometry) {
        errors.push(ExperimentModelError::InvalidGeometry);
    }
    if !spec.simulation_dt.is_finite() || spec.simulation_dt <= 0.0 {
        errors.push(ExperimentModelError::InvalidSimulationDt);
    }
    if let Some(tiling) = &spec.tiling {
        if !tiling.translation_a.x.is_finite()
            || !tiling.translation_a.y.is_finite()
            || !tiling.translation_b.x.is_finite()
            || !tiling.translation_b.y.is_finite()
            || tiling.translation_a.cross(tiling.translation_b).abs() <= 1e-12
        {
            errors.push(ExperimentModelError::InvalidTiling(
                "translation vectors must be finite and non-collinear".into(),
            ));
        }
        let mut prototype_ids = BTreeSet::new();
        for prototype in &tiling.prototypes {
            if !prototype_ids.insert(prototype.id) {
                errors.push(ExperimentModelError::InvalidTiling(format!(
                    "duplicate prototype {:?}",
                    prototype.id
                )));
            }
            match prototype_vertices(&prototype.shape) {
                Ok(vertices) => {
                    for issue in validate_polygon(&vertices) {
                        errors.push(ExperimentModelError::InvalidTiling(format!(
                            "prototype {:?}: {}",
                            prototype.id, issue.message
                        )));
                    }
                }
                Err(issues) => errors.extend(issues.into_iter().map(|issue| {
                    ExperimentModelError::InvalidTiling(format!(
                        "prototype {:?}: {}",
                        prototype.id, issue.message
                    ))
                })),
            }
        }
        let prototype_ids: BTreeSet<_> = tiling.prototypes.iter().map(|p| p.id).collect();
        let mut tile_ids = BTreeSet::new();
        for tile in &tiling.instances {
            if !tile_ids.insert(tile.id) {
                errors.push(ExperimentModelError::InvalidTiling(format!(
                    "duplicate tile {:?}",
                    tile.id
                )));
            }
            if !prototype_ids.contains(&tile.prototype) {
                errors.push(ExperimentModelError::InvalidTiling(format!(
                    "tile {:?} references missing prototype {:?}",
                    tile.id, tile.prototype
                )));
            }
            if !tile.transform.rotation.is_finite()
                || !tile.transform.translation.x.is_finite()
                || !tile.transform.translation.y.is_finite()
            {
                errors.push(ExperimentModelError::InvalidTiling(format!(
                    "tile {:?} has non-finite transform",
                    tile.id
                )));
            }
        }
        if tiling.instances.is_empty() {
            errors.push(ExperimentModelError::InvalidTiling(
                "at least one tile instance is required".into(),
            ));
        }
    }
    if spec.channels.is_empty() {
        errors.push(ExperimentModelError::EmptyChannels);
    }

    let mut channel_ids = BTreeSet::new();
    let mut channel_names = BTreeSet::new();
    for channel in &spec.channels {
        if !channel_ids.insert(channel.id) {
            errors.push(ExperimentModelError::DuplicateChannelId(channel.id));
        }
        if channel.name.trim().is_empty() || !channel_names.insert(channel.name.as_str()) {
            errors.push(ExperimentModelError::InvalidChannelName(
                channel.name.clone(),
            ));
        }
        if let Some(cell_count) = tile_count {
            let basis_count = spec.basis_ids().len();
            let expanded = cell_count.checked_mul(basis_count);
            if channel.initial.len() != cell_count && expanded != Some(channel.initial.len()) {
                errors.push(ExperimentModelError::InvalidInitialLength {
                    channel: channel.id,
                    expected: expanded.unwrap_or(cell_count),
                    actual: channel.initial.len(),
                });
            }
        }
        if channel.initial.iter().any(|value| !value.is_finite()) {
            errors.push(ExperimentModelError::NonFiniteChannel(channel.id));
        }
        if !channel.boundary_constant.is_finite() {
            errors.push(ExperimentModelError::NonFiniteBoundary(channel.id));
        }
        if !channel.display.opacity.is_finite() || !(0.0..=1.0).contains(&channel.display.opacity) {
            errors.push(ExperimentModelError::InvalidOpacity(channel.id));
        }
    }

    let channels = spec
        .channels
        .iter()
        .map(|channel| (channel.id, channel))
        .collect::<BTreeMap<_, _>>();
    let has_legacy = !spec.kernels.is_empty() || !spec.growth.is_empty();
    let has_normalized = !spec.rules.is_empty();
    if has_legacy && has_normalized {
        errors.push(ExperimentModelError::AmbiguousRuleRepresentations);
    }
    let validate_legacy = !has_normalized;

    let mut kernel_ids = BTreeSet::new();
    let mut kernel_symbols = BTreeSet::new();
    for kernel in spec.kernels.iter().filter(|_| validate_legacy) {
        if !kernel_ids.insert(kernel.id) {
            errors.push(ExperimentModelError::DuplicateKernelId(kernel.id));
        }
        if kernel.symbol.trim().is_empty()
            || kernel.symbol == "self"
            || !kernel_symbols.insert(kernel.symbol.as_str())
        {
            errors.push(ExperimentModelError::InvalidKernelSymbol(
                kernel.symbol.clone(),
            ));
        }
        if !channels.contains_key(&kernel.source) {
            errors.push(ExperimentModelError::MissingKernelChannel {
                kernel: kernel.id,
                role: "source",
                channel: kernel.source,
            });
        }
        match channels.get(&kernel.target) {
            None => errors.push(ExperimentModelError::MissingKernelChannel {
                kernel: kernel.id,
                role: "target",
                channel: kernel.target,
            }),
            Some(channel) if channel.frozen => {
                errors.push(ExperimentModelError::FrozenKernelTarget {
                    kernel: kernel.id,
                    target: kernel.target,
                })
            }
            Some(_) => {}
        }
        if let Err(error) = kernel.definition.build() {
            errors.push(ExperimentModelError::InvalidKernel {
                kernel: kernel.id,
                reason: error.to_string(),
            });
        }
    }

    let mut growth_targets = BTreeSet::new();
    for growth in spec.growth.iter().filter(|_| validate_legacy) {
        if !growth_targets.insert(growth.target) {
            errors.push(ExperimentModelError::DuplicateGrowthTarget(growth.target));
        }
        match channels.get(&growth.target) {
            None => errors.push(ExperimentModelError::MissingGrowthTarget(growth.target)),
            Some(channel) if channel.frozen => {
                errors.push(ExperimentModelError::FrozenGrowthTarget(growth.target))
            }
            Some(_) => {}
        }
        let mut expected = spec
            .kernels
            .iter()
            .filter(|kernel| kernel.target == growth.target)
            .map(|kernel| kernel.id)
            .collect::<Vec<_>>();
        expected.sort_unstable();
        let mut actual = growth.kernel_inputs.clone();
        actual.sort_unstable();
        if actual != expected || growth.kernel_inputs != actual {
            errors.push(ExperimentModelError::GrowthKernelMismatch {
                target: growth.target,
                expected,
                actual: growth.kernel_inputs.clone(),
            });
        }
        for (name, value) in &growth.parameters {
            if name.trim().is_empty() || name == "self" || !value.is_finite() {
                errors.push(ExperimentModelError::InvalidGrowthParameter {
                    target: growth.target,
                    parameter: name.clone(),
                });
            }
        }
        if growth.source.trim().is_empty() {
            errors.push(ExperimentModelError::EmptyGrowthSource(growth.target));
        }
    }
    for channel in spec
        .channels
        .iter()
        .filter(|channel| validate_legacy && !channel.frozen)
    {
        if !growth_targets.contains(&channel.id) {
            errors.push(ExperimentModelError::MissingGrowthProgram(channel.id));
        }
    }

    if !spec.rules.is_empty()
        && let Err(rule_errors) = spec.rules.validate(&spec.basis_ids(), &spec.channels)
    {
        errors.extend(
            rule_errors
                .into_iter()
                .map(|error| ExperimentModelError::InvalidRules(error.to_string())),
        );
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_channel_lenia_can_represent_an_uncommitted_empty_tiling() {
        let model = ExperimentSpec::single_channel_lenia(32, 24);
        assert!(model.tiling.is_none());
        assert_eq!(model.channels.len(), 1);
        assert_eq!(model.kernels.len(), 1);
    }
    use crate::sim::tiling::{TilingPreset, build_preset};

    #[test]
    fn default_model_is_single_channel_and_runnable() {
        let model = ExperimentSpec::single_channel_lenia(32, 24);
        assert_eq!(model.channels.len(), 1);
        assert_eq!(model.channels[0].initial.len(), 32 * 24);
        assert!(validate_structure(&model).is_ok());
    }

    #[test]
    fn default_has_one_channel_one_basis_one_kernel() {
        let spec = ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .unwrap();
        let basis = spec.basis_ids();
        assert_eq!(basis.len(), 1);
        let binding = spec.rules.binding(basis[0], spec.channels[0].id).unwrap();
        assert_eq!(spec.rules.get(binding.rule_set).unwrap().kernels.len(), 1);
    }

    #[test]
    fn growth_inputs_are_exactly_targeting_kernels() {
        let mut model = ExperimentSpec::single_channel_lenia(4, 4);
        let channel = model.channels[0].id;
        model
            .kernels
            .push(KernelSlot::identity(KernelId(1), "crowd", channel, channel));
        model.growth[0].kernel_inputs.push(KernelId(999));
        let errors = validate_structure(&model).unwrap_err();
        assert!(errors.iter().any(|error| matches!(
            error,
            ExperimentModelError::GrowthKernelMismatch { target, .. } if *target == channel
        )));
    }

    #[test]
    fn frozen_target_is_rejected_but_frozen_source_is_allowed() {
        let mut model = ExperimentSpec::single_channel_lenia(2, 2);
        let frozen = model.add_channel("environment", true);
        let active = model.channels[0].id;
        model
            .kernels
            .push(KernelSlot::identity(KernelId(7), "signal", frozen, active));
        model.growth[0].kernel_inputs.push(KernelId(7));
        assert!(validate_structure(&model).is_ok());
        model.kernels[0].target = frozen;
        assert!(validate_structure(&model).is_err());
    }

    #[test]
    fn legacy_global_rule_is_shared_by_all_existing_bases() {
        let mut legacy = ExperimentSpec::single_channel_lenia(4, 4);
        legacy.tiling = Some(build_preset(TilingPreset::OctagonSquare, 1.0));

        let normalized = legacy.normalize_rules().unwrap();
        let ids = normalized
            .rules
            .bindings
            .iter()
            .map(|binding| binding.rule_set)
            .collect::<BTreeSet<_>>();
        assert_eq!(ids.len(), 1);
        assert!(normalized.kernels.is_empty());
        assert!(normalized.growth.is_empty());
    }

    #[test]
    fn normalized_and_legacy_rules_are_ambiguous() {
        let normalized = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let mut ambiguous = normalized.clone();
        ambiguous.kernels = ExperimentSpec::single_channel_lenia(4, 4).kernels;

        assert!(
            ambiguous
                .normalize_rules()
                .unwrap_err()
                .iter()
                .any(|error| {
                    matches!(error, ExperimentModelError::AmbiguousRuleRepresentations)
                })
        );
    }

    #[test]
    fn normalized_ron_omits_empty_legacy_rule_vectors() {
        let normalized = ExperimentSpec::single_channel_lenia(4, 4)
            .normalize_rules()
            .unwrap();
        let encoded = ron::ser::to_string(&normalized).unwrap();
        assert!(!encoded.contains("kernels:[]"));
        assert!(!encoded.contains("growth:[]"));
        assert_eq!(
            ron::from_str::<ExperimentSpec>(&encoded).unwrap(),
            normalized
        );
    }

    #[test]
    fn legacy_kernel_storage_order_normalizes_to_growth_input_order() {
        let mut legacy = ExperimentSpec::single_channel_lenia(2, 2);
        let channel = legacy.channels[0].id;
        legacy.kernels[0].id = KernelId(5);
        legacy
            .kernels
            .push(KernelSlot::identity(KernelId(3), "inner", channel, channel));
        legacy.growth[0].kernel_inputs = vec![KernelId(3), KernelId(5)];

        let normalized = legacy.normalize_rules().unwrap();
        assert_eq!(
            normalized.rules.sets[0]
                .kernels
                .iter()
                .map(|kernel| kernel.id)
                .collect::<Vec<_>>(),
            vec![KernelId(3), KernelId(5)]
        );
        assert_eq!(
            normalized.rules.sets[0].growth.kernel_inputs,
            vec![KernelId(3), KernelId(5)]
        );
    }
}
