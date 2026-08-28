//! Pure experiment transforms for the kernel lifecycle.
//!
//! A kernel is not an isolated grid of numbers: its symbol may appear in a
//! growth program, and removing it can make that program stop compiling. These
//! transforms compute the whole consequence of an edit and hand back one draft,
//! so a caller commits a complete change or none of it.

use std::collections::BTreeMap;

use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
use crate::sim::experiment_model::{ChannelId, ExperimentSpec, KernelId, KernelSlot};
use crate::sim::kernel::KernelValues;
use crate::sim::ruleset::{BindingKey, KernelSpatialDefinition, RuleKernel, RuleSetId};
use crate::sim::tiling::BasisId;

/// One kernel as the editor shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct KernelCardModel {
    pub id: KernelId,
    /// Position in the binding, counted from one for display.
    pub ordinal: usize,
    pub symbol: String,
    pub name: String,
    pub source_channel: ChannelId,
    pub width: usize,
    pub height: usize,
    /// Cells the kernel samples: the support the Support tool edits, which is
    /// what the canvas legend and the inspector both call active.
    pub support_cells: usize,
    /// Cells that carry a weight other than zero. A cell can be in the support
    /// and still contribute nothing, which is a state the canvas draws.
    pub weighted_cells: usize,
    pub periodic: bool,
    pub selected: bool,
}

/// Which rule-set a binding resolves to, when the experiment uses the
/// basis-aware rule model.
pub fn rule_set_for(spec: &ExperimentSpec, binding: BindingKey) -> Option<RuleSetId> {
    spec.rules
        .binding(binding.basis, binding.output)
        .map(|entry| entry.rule_set)
}

/// The kernels of one binding, in the order the growth signature sees them.
pub fn binding_kernels(
    spec: &ExperimentSpec,
    binding: BindingKey,
    selected: Option<KernelId>,
) -> Vec<KernelCardModel> {
    if let Some(rule_set) = rule_set_for(spec, binding)
        && let Some(rule) = spec.rules.get(rule_set)
    {
        return rule
            .kernels
            .iter()
            .enumerate()
            .map(|(index, kernel)| {
                let (width, height, support, weighted, periodic) = shape_of(&kernel.spatial);
                KernelCardModel {
                    id: kernel.id,
                    ordinal: index + 1,
                    symbol: kernel.symbol.clone(),
                    name: kernel.name.clone(),
                    source_channel: kernel.source_channel,
                    width,
                    height,
                    support_cells: support,
                    weighted_cells: weighted,
                    periodic,
                    selected: selected == Some(kernel.id),
                }
            })
            .collect();
    }
    // Legacy model: kernels live on the experiment and target a channel.
    spec.kernels
        .iter()
        .filter(|kernel| kernel.target == binding.output)
        .enumerate()
        .map(|(index, kernel)| {
            let definition = &kernel.definition;
            KernelCardModel {
                id: kernel.id,
                ordinal: index + 1,
                symbol: kernel.symbol.clone(),
                name: definition.name.clone(),
                source_channel: kernel.source,
                width: definition.width,
                height: definition.height,
                support_cells: raster_support(definition),
                weighted_cells: raster_weighted(definition),
                periodic: false,
                selected: selected == Some(kernel.id),
            }
        })
        .collect()
}

fn shape_of(spatial: &KernelSpatialDefinition) -> (usize, usize, usize, usize, bool) {
    match spatial {
        KernelSpatialDefinition::Raster(definition) => (
            definition.width,
            definition.height,
            raster_support(definition),
            raster_weighted(definition),
            false,
        ),
        KernelSpatialDefinition::Periodic(definition) => {
            let in_mask = |plane: &crate::sim::basis_kernel::BasisWeightPlane, index: usize| {
                plane
                    .mask
                    .as_ref()
                    .is_none_or(|mask| mask.get(index).copied().unwrap_or(true))
            };
            let support = definition
                .planes
                .values()
                .map(|plane| {
                    plane
                        .values
                        .iter()
                        .enumerate()
                        .filter(|(index, _)| in_mask(plane, *index))
                        .count()
                })
                .sum();
            let weighted = definition
                .planes
                .values()
                .map(|plane| {
                    plane
                        .values
                        .iter()
                        .enumerate()
                        .filter(|(index, value)| **value != 0.0 && in_mask(plane, *index))
                        .count()
                })
                .sum();
            (definition.width, definition.height, support, weighted, true)
        }
    }
}

/// Cells the kernel samples.
///
/// The support: whether the cell is in the mask, not whether its weight happens
/// to be non-zero. The canvas draws a masked-in cell holding zero as
/// `active zero` and the inspector reads it back as active, so a count that
/// excluded it would contradict the same screen it sits on — and it would never
/// move when the Support tool switched a cell on.
fn raster_support(definition: &crate::sim::kernel::KernelDefinition) -> usize {
    match &definition.values {
        KernelValues::Explicit(values) => values
            .iter()
            .enumerate()
            .filter(|(index, _)| in_raster_mask(definition, *index))
            .count(),
        KernelValues::Expression(_) => definition
            .mask
            .as_ref()
            .map(|mask| mask.iter().filter(|active| **active).count())
            .unwrap_or(definition.width * definition.height),
    }
}

/// Cells that carry a weight other than zero.
fn raster_weighted(definition: &crate::sim::kernel::KernelDefinition) -> usize {
    match &definition.values {
        KernelValues::Explicit(values) => values
            .iter()
            .enumerate()
            .filter(|(index, value)| **value != 0.0 && in_raster_mask(definition, *index))
            .count(),
        // An expression fills its whole stencil unless masked out.
        KernelValues::Expression(_) => raster_support(definition),
    }
}

fn in_raster_mask(definition: &crate::sim::kernel::KernelDefinition, index: usize) -> bool {
    definition
        .mask
        .as_ref()
        .is_none_or(|mask| mask.get(index).copied().unwrap_or(true))
}

/// Append a kernel to a binding and give it a place in the growth signature.
pub fn add_kernel(
    spec: &ExperimentSpec,
    binding: BindingKey,
) -> Result<(ExperimentSpec, KernelId), String> {
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        let id = KernelId(
            rule.kernels
                .iter()
                .map(|kernel| kernel.id.0)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("kernel id exhausted")?,
        );
        let symbol = format!("k{}", id.0);
        // A new kernel copies the shape of the one already there, so it lands in
        // the same coordinate system the user is looking at.
        let spatial = rule
            .kernels
            .first()
            .map(|kernel| identity_like(&kernel.spatial))
            .unwrap_or_else(|| {
                KernelSpatialDefinition::Raster(
                    KernelSlot::identity(id, &symbol, binding.output, binding.output).definition,
                )
            });
        rule.kernels.push(RuleKernel {
            id,
            symbol: symbol.clone(),
            name: symbol,
            source_channel: binding.output,
            spatial,
        });
        rule.growth.kernel_inputs.push(id);
        rule.validate().map_err(|error| error.to_string())?;
        return Ok((next, id));
    }

    let id = KernelId(
        next.kernels
            .iter()
            .map(|kernel| kernel.id.0.saturating_add(1))
            .max()
            .unwrap_or(0),
    );
    let symbol = format!("k{}", id.0);
    next.kernels.push(KernelSlot::identity(
        id,
        &symbol,
        binding.output,
        binding.output,
    ));
    if let Some(growth) = next
        .growth
        .iter_mut()
        .find(|growth| growth.target == binding.output)
    {
        growth.kernel_inputs.push(id);
    }
    Ok((next, id))
}

/// A kernel shaped like `template` but holding only the identity: a single
/// weight at the anchor.
///
/// It is not empty, because a kernel with no weight at all fails sum
/// normalization and could not be committed. The identity is the smallest
/// valid kernel and says something true — this one reads the cell itself — so
/// the user starts from a definite state rather than from an error, and never
/// from a silent copy of the neighbouring kernel's support.
fn identity_like(template: &KernelSpatialDefinition) -> KernelSpatialDefinition {
    match template {
        KernelSpatialDefinition::Raster(definition) => {
            let mut fresh = definition.clone();
            let mut values = vec![0.0; fresh.width * fresh.height];
            if let Some(index) =
                anchor_index(fresh.width, fresh.height, fresh.anchor_x, fresh.anchor_y)
            {
                values[index] = 1.0;
            }
            fresh.values = KernelValues::Explicit(values);
            fresh.mask = None;
            KernelSpatialDefinition::Raster(fresh)
        }
        KernelSpatialDefinition::Periodic(definition) => {
            let anchor = anchor_index(
                definition.width,
                definition.height,
                definition.anchor_x,
                definition.anchor_y,
            );
            let planes = definition
                .planes
                .keys()
                .map(|basis| {
                    let mut values = vec![0.0; definition.width * definition.height];
                    if let Some(index) = anchor {
                        values[index] = 1.0;
                    }
                    (*basis, BasisWeightPlane { values, mask: None })
                })
                .collect::<BTreeMap<_, _>>();
            KernelSpatialDefinition::Periodic(PeriodicKernelDefinition {
                width: definition.width,
                height: definition.height,
                anchor_x: definition.anchor_x,
                anchor_y: definition.anchor_y,
                planes,
            })
        }
    }
}

fn anchor_index(width: usize, height: usize, anchor_x: usize, anchor_y: usize) -> Option<usize> {
    (anchor_x < width && anchor_y < height).then(|| anchor_y * width + anchor_x)
}

/// How a growth program would have to change for a removal to succeed.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceRewrite {
    pub symbol: String,
    pub before: String,
    pub after: String,
}

/// The consequence of removing one kernel.
#[derive(Clone, Debug, PartialEq)]
pub struct RemovalPlan {
    /// The draft that results, with any rewrite already applied.
    pub spec: ExperimentSpec,
    /// Present when the growth source referenced the kernel and had to change.
    /// A caller shows this before committing, never after.
    pub rewrite: Option<SourceRewrite>,
}

/// Work out what removing `kernel` would do, without changing anything.
///
/// A referenced kernel is not refused outright: the reference can be replaced
/// with zero, which is a real answer the user may want. The exact rewrite is
/// returned so the choice is made with the new source in view.
pub fn plan_removal(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
) -> Result<RemovalPlan, String> {
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        if rule.kernels.len() <= 1 {
            return Err("a rule-set must keep at least one kernel".into());
        }
        let position = rule
            .kernels
            .iter()
            .position(|entry| entry.id == kernel)
            .ok_or("that kernel is not part of this binding")?;
        let removed = rule.kernels.remove(position);
        rule.growth.kernel_inputs.retain(|id| *id != removed.id);
        let rewrite = rewrite_if_referenced(&rule.growth.source, &removed.symbol);
        if let Some(rewrite) = &rewrite {
            rule.growth.source = rewrite.after.clone();
        }
        rule.validate().map_err(|error| error.to_string())?;
        return Ok(RemovalPlan {
            spec: next,
            rewrite,
        });
    }

    let candidates: Vec<_> = next
        .kernels
        .iter()
        .enumerate()
        .filter(|(_, entry)| entry.target == binding.output)
        .map(|(position, entry)| (position, entry.id))
        .collect();
    if candidates.len() <= 1 {
        return Err("a channel must keep at least one kernel".into());
    }
    let position = candidates
        .iter()
        .find(|(_, id)| *id == kernel)
        .map(|(position, _)| *position)
        .ok_or("that kernel is not part of this channel")?;
    let removed = next.kernels.remove(position);
    let mut rewrite = None;
    if let Some(growth) = next
        .growth
        .iter_mut()
        .find(|growth| growth.target == binding.output)
    {
        growth.kernel_inputs.retain(|id| *id != removed.id);
        rewrite = rewrite_if_referenced(&growth.source, &removed.symbol);
        if let Some(rewrite) = &rewrite {
            growth.source = rewrite.after.clone();
        }
    }
    Ok(RemovalPlan {
        spec: next,
        rewrite,
    })
}

fn rewrite_if_referenced(source: &str, symbol: &str) -> Option<SourceRewrite> {
    let after = replace_symbol(source, symbol, "0.0");
    (after != source).then(|| SourceRewrite {
        symbol: symbol.to_string(),
        before: source.to_string(),
        after,
    })
}

/// Replace whole-identifier occurrences of `symbol`.
///
/// Substring replacement would corrupt neighbouring names: rewriting `k1` in
/// `k10 + k1` must not touch `k10`. An identifier ends where a character that
/// cannot continue one begins.
pub fn replace_symbol(source: &str, symbol: &str, replacement: &str) -> String {
    if symbol.is_empty() {
        return source.to_string();
    }
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        if source[index..].starts_with(symbol) {
            let before_ok = index == 0 || !is_identifier_byte(bytes[index - 1]);
            let end = index + symbol.len();
            let after_ok = end >= source.len() || !is_identifier_byte(bytes[end]);
            if before_ok && after_ok {
                out.push_str(replacement);
                index = end;
                continue;
            }
        }
        let ch = source[index..].chars().next().expect("index is on a char");
        out.push(ch);
        index += ch.len_utf8();
    }
    out
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// Flatten one kernel into the grid the canvas draws.
///
/// The canvas never learns which rule model an experiment uses: both collapse
/// to weights, a support mask and an anchor, which is all a stencil is.
pub fn stencil_of(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
    basis: BasisId,
) -> Option<crate::gui::canvas::kernel::KernelStencil> {
    use crate::gui::canvas::kernel::KernelStencil;

    let spatial = spatial_of(spec, binding, kernel)?;
    match spatial {
        KernelSpatialDefinition::Raster(definition) => {
            let cells = definition.width * definition.height;
            let weights = match &definition.values {
                KernelValues::Explicit(values) => {
                    let mut values = values.clone();
                    values.resize(cells, 0.0);
                    values
                }
                KernelValues::Expression(_) => definition.build().ok()?.values.clone(),
            };
            let active = definition
                .mask
                .clone()
                .map(|mut mask| {
                    mask.resize(cells, true);
                    mask
                })
                .unwrap_or_else(|| vec![true; cells]);
            Some(KernelStencil {
                width: definition.width,
                height: definition.height,
                anchor_x: definition.anchor_x,
                anchor_y: definition.anchor_y,
                weights,
                active,
            })
        }
        KernelSpatialDefinition::Periodic(definition) => {
            let cells = definition.width * definition.height;
            // A periodic kernel holds one plane per source basis; the canvas
            // shows the plane for the basis being edited.
            let plane = definition.planes.get(&basis);
            let weights = plane
                .map(|plane| {
                    let mut values = plane.values.clone();
                    values.resize(cells, 0.0);
                    values
                })
                .unwrap_or_else(|| vec![0.0; cells]);
            let active = plane
                .and_then(|plane| plane.mask.clone())
                .map(|mut mask| {
                    mask.resize(cells, true);
                    mask
                })
                .unwrap_or_else(|| vec![true; cells]);
            Some(KernelStencil {
                width: definition.width,
                height: definition.height,
                anchor_x: definition.anchor_x,
                anchor_y: definition.anchor_y,
                weights,
                active,
            })
        }
    }
}

fn spatial_of(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
) -> Option<KernelSpatialDefinition> {
    if let Some(rule_set) = rule_set_for(spec, binding)
        && let Some(rule) = spec.rules.get(rule_set)
    {
        return rule
            .kernels
            .iter()
            .find(|entry| entry.id == kernel)
            .map(|entry| entry.spatial.clone());
    }
    spec.kernels
        .iter()
        .find(|entry| entry.id == kernel)
        .map(|entry| KernelSpatialDefinition::Raster(entry.definition.clone()))
}

/// Change which channel a kernel reads.
pub fn set_source(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
    source: ChannelId,
) -> Result<ExperimentSpec, String> {
    if !spec.channels.iter().any(|channel| channel.id == source) {
        return Err("that channel does not exist".into());
    }
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        let entry = rule
            .kernels
            .iter_mut()
            .find(|entry| entry.id == kernel)
            .ok_or("that kernel is not part of this binding")?;
        entry.source_channel = source;
        rule.validate().map_err(|error| error.to_string())?;
        return Ok(next);
    }
    let slot = next
        .kernels
        .iter_mut()
        .find(|entry| entry.id == kernel)
        .ok_or("that kernel is missing")?;
    slot.source = source;
    Ok(next)
}

/// Set one weight, in whichever representation the kernel uses.
pub fn set_weight(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
    basis: BasisId,
    x: usize,
    y: usize,
    value: f32,
) -> Result<ExperimentSpec, String> {
    if !value.is_finite() {
        return Err("a kernel weight must be finite".into());
    }
    edit_kernel(spec, binding, kernel, |spatial| match spatial {
        KernelSpatialDefinition::Raster(definition) => {
            let index = index_of(definition.width, definition.height, x, y)?;
            let mut values = match &definition.values {
                KernelValues::Explicit(values) => values.clone(),
                // Painting a generated kernel makes it an explicit one: the
                // expression can no longer describe what is on screen.
                KernelValues::Expression(_) => definition
                    .build()
                    .map_err(|error| error.to_string())?
                    .values
                    .clone(),
            };
            values.resize(definition.width * definition.height, 0.0);
            values[index] = value;
            definition.values = KernelValues::Explicit(values);
            Ok(())
        }
        KernelSpatialDefinition::Periodic(definition) => {
            let index = index_of(definition.width, definition.height, x, y)?;
            let plane = definition
                .planes
                .entry(basis)
                .or_insert_with(|| BasisWeightPlane {
                    values: vec![0.0; definition.width * definition.height],
                    mask: None,
                });
            plane
                .values
                .resize(definition.width * definition.height, 0.0);
            plane.values[index] = value;
            Ok(())
        }
    })
}

/// Turn one cell on or off. An inactive cell contributes nothing regardless of
/// the weight it still holds, which is what makes Support a separate tool.
pub fn set_active(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
    basis: BasisId,
    x: usize,
    y: usize,
    active: bool,
) -> Result<ExperimentSpec, String> {
    edit_kernel(spec, binding, kernel, |spatial| match spatial {
        KernelSpatialDefinition::Raster(definition) => {
            let index = index_of(definition.width, definition.height, x, y)?;
            let mask = definition
                .mask
                .get_or_insert_with(|| vec![true; definition.width * definition.height]);
            mask.resize(definition.width * definition.height, true);
            mask[index] = active;
            Ok(())
        }
        KernelSpatialDefinition::Periodic(definition) => {
            let index = index_of(definition.width, definition.height, x, y)?;
            let width = definition.width;
            let height = definition.height;
            let plane = definition
                .planes
                .entry(basis)
                .or_insert_with(|| BasisWeightPlane {
                    values: vec![0.0; width * height],
                    mask: None,
                });
            let mask = plane.mask.get_or_insert_with(|| vec![true; width * height]);
            mask.resize(width * height, true);
            mask[index] = active;
            Ok(())
        }
    })
}

fn index_of(width: usize, height: usize, x: usize, y: usize) -> Result<usize, String> {
    if x >= width || y >= height {
        return Err(format!(
            "({x}, {y}) is outside the {width}x{height} stencil"
        ));
    }
    Ok(y * width + x)
}

/// Apply an edit to one kernel of a binding, in either rule model.
fn edit_kernel(
    spec: &ExperimentSpec,
    binding: BindingKey,
    kernel: KernelId,
    edit: impl FnOnce(&mut KernelSpatialDefinition) -> Result<(), String>,
) -> Result<ExperimentSpec, String> {
    let mut next = spec.clone();
    if !next.rules.is_empty() {
        let rule_set = next
            .rules
            .detach(binding)
            .map_err(|error| error.to_string())?;
        let rule = next
            .rules
            .get_mut(rule_set)
            .ok_or("the selected rule-set is missing")?;
        let entry = rule
            .kernels
            .iter_mut()
            .find(|entry| entry.id == kernel)
            .ok_or("that kernel is not part of this binding")?;
        edit(&mut entry.spatial)?;
        rule.validate().map_err(|error| error.to_string())?;
        return Ok(next);
    }
    let slot = next
        .kernels
        .iter_mut()
        .find(|entry| entry.id == kernel)
        .ok_or("that kernel is missing")?;
    let mut spatial = KernelSpatialDefinition::Raster(slot.definition.clone());
    edit(&mut spatial)?;
    let KernelSpatialDefinition::Raster(definition) = spatial else {
        return Err("a legacy kernel stays a raster kernel".into());
    };
    slot.definition = definition;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The legacy model, where kernels hang off the experiment.
    fn spec() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
    }

    /// The basis-aware rule model, which is what a migrated experiment uses.
    fn normalized() -> ExperimentSpec {
        ExperimentSpec::single_channel_lenia(8, 8)
            .normalize_rules()
            .expect("the fixture normalizes")
    }

    fn binding(spec: &ExperimentSpec) -> BindingKey {
        BindingKey {
            basis: spec.basis_ids().first().copied().unwrap_or(BasisId(0)),
            output: spec.channels[0].id,
        }
    }

    #[test]
    fn a_symbol_is_replaced_only_as_a_whole_identifier() {
        assert_eq!(replace_symbol("k1 + k10", "k1", "0.0"), "0.0 + k10");
        assert_eq!(replace_symbol("k10 + k1", "k1", "0.0"), "k10 + 0.0");
        assert_eq!(replace_symbol("ak1 + k1b", "k1", "0.0"), "ak1 + k1b");
        assert_eq!(
            replace_symbol("gauss(k1,0.5)", "k1", "0.0"),
            "gauss(0.0,0.5)"
        );
        assert_eq!(replace_symbol("k_1 + k1", "k1", "0.0"), "k_1 + 0.0");
    }

    #[test]
    fn replacing_a_symbol_that_is_absent_changes_nothing() {
        assert_eq!(replace_symbol("self * 2.0", "k7", "0.0"), "self * 2.0");
        assert_eq!(replace_symbol("anything", "", "0.0"), "anything");
    }

    #[test]
    fn a_binding_lists_its_kernels_in_signature_order() {
        let spec = spec();
        let cards = binding_kernels(&spec, binding(&spec), None);
        assert!(!cards.is_empty());
        for (index, card) in cards.iter().enumerate() {
            assert_eq!(card.ordinal, index + 1);
        }
    }

    #[test]
    fn adding_a_kernel_extends_the_binding_and_the_signature() {
        let spec = spec();
        let before = binding_kernels(&spec, binding(&spec), None).len();
        let (next, id) = add_kernel(&spec, binding(&spec)).unwrap();
        let cards = binding_kernels(&next, binding(&next), Some(id));
        assert_eq!(cards.len(), before + 1);
        assert!(cards.iter().any(|card| card.id == id && card.selected));
    }

    #[test]
    fn a_new_kernel_starts_as_the_identity_not_a_copy_of_its_neighbour() {
        let spec = normalized();
        let key = binding(&spec);
        let template = binding_kernels(&spec, key, None)[0].weighted_cells;
        assert!(template > 1, "the fixture kernel has real support");

        let (next, id) = add_kernel(&spec, key).unwrap();
        let card = binding_kernels(&next, binding(&next), None)
            .into_iter()
            .find(|card| card.id == id)
            .unwrap();
        assert_eq!(
            card.weighted_cells, 1,
            "a fresh kernel holds only the identity, never the neighbour's weights"
        );
    }

    #[test]
    fn removing_an_unreferenced_kernel_needs_no_rewrite() {
        let spec = spec();
        let (with_extra, id) = add_kernel(&spec, binding(&spec)).unwrap();
        let plan = plan_removal(&with_extra, binding(&with_extra), id).unwrap();
        assert_eq!(plan.rewrite, None);
        assert!(
            !binding_kernels(&plan.spec, binding(&plan.spec), None)
                .iter()
                .any(|card| card.id == id)
        );
    }

    #[test]
    fn removing_a_referenced_kernel_reports_the_exact_rewrite() {
        let spec = normalized();
        let (mut with_extra, id) = add_kernel(&spec, binding(&spec)).unwrap();
        let key = binding(&with_extra);
        let symbol = binding_kernels(&with_extra, key, None)
            .into_iter()
            .find(|card| card.id == id)
            .unwrap()
            .symbol;
        let rule_set = rule_set_for(&with_extra, key).unwrap();
        let rule = with_extra.rules.get_mut(rule_set).unwrap();
        rule.growth.source = format!("{symbol} * 2.0");

        let plan = plan_removal(&with_extra, key, id).unwrap();
        let rewrite = plan.rewrite.expect("a referenced symbol must be reported");
        assert_eq!(rewrite.symbol, symbol);
        assert_eq!(rewrite.before, format!("{symbol} * 2.0"));
        assert_eq!(rewrite.after, "0.0 * 2.0");
        // The plan carries the rewrite already applied, so the caller commits
        // the kernel removal and the source change as one draft.
        let rule_set = rule_set_for(&plan.spec, key).unwrap();
        assert_eq!(
            plan.spec.rules.get(rule_set).unwrap().growth.source,
            "0.0 * 2.0"
        );
    }

    #[test]
    fn planning_a_removal_never_touches_the_input() {
        let spec = spec();
        let (with_extra, id) = add_kernel(&spec, binding(&spec)).unwrap();
        let before = with_extra.clone();
        let _ = plan_removal(&with_extra, binding(&with_extra), id).unwrap();
        assert_eq!(with_extra, before);
    }

    #[test]
    fn the_last_kernel_of_a_binding_cannot_be_removed() {
        let spec = spec();
        let key = binding(&spec);
        let only = binding_kernels(&spec, key, None)[0].id;
        let error = plan_removal(&spec, key, only).unwrap_err();
        assert!(error.contains("at least one kernel"), "{error}");
    }

    #[test]
    fn a_stencil_matches_the_kernel_it_flattens() {
        let spec = normalized();
        let key = binding(&spec);
        let card = binding_kernels(&spec, key, None)[0].clone();
        let stencil = stencil_of(&spec, key, card.id, key.basis).expect("the kernel exists");
        assert_eq!(stencil.width, card.width);
        assert_eq!(stencil.height, card.height);
        assert_eq!(stencil.weights.len(), stencil.width * stencil.height);
        assert_eq!(stencil.active.len(), stencil.weights.len());
        assert!(stencil.anchor_x < stencil.width);
        assert!(stencil.anchor_y < stencil.height);
    }

    #[test]
    fn a_painted_weight_shows_up_in_the_stencil_at_the_same_cell() {
        let spec = normalized();
        let key = binding(&spec);
        let kernel = binding_kernels(&spec, key, None)[0].id;
        let next = set_weight(&spec, key, kernel, key.basis, 2, 1, -0.25).unwrap();
        let stencil = stencil_of(&next, key, kernel, key.basis).unwrap();
        assert!((stencil.weight(2, 1) + 0.25).abs() < 1e-6);
    }

    #[test]
    fn a_kernel_reads_the_channel_it_is_pointed_at_and_refuses_an_unknown_one() {
        let spec = normalized();
        let key = binding(&spec);
        let kernel = binding_kernels(&spec, key, None)[0].id;
        let channel = spec.channels[0].id;
        let next = set_source(&spec, key, kernel, channel).unwrap();
        assert_eq!(binding_kernels(&next, key, None)[0].source_channel, channel);
        assert!(set_source(&spec, key, kernel, ChannelId(99)).is_err());
    }

    #[test]
    fn a_weight_lands_on_the_cell_it_names_and_out_of_range_is_refused() {
        let spec = spec();
        let key = binding(&spec);
        let kernel = binding_kernels(&spec, key, None)[0].id;
        let basis = key.basis;
        let next = set_weight(&spec, key, kernel, basis, 1, 2, 0.75).unwrap();
        assert_ne!(next, spec);

        let card = binding_kernels(&next, key, None)[0].clone();
        let error = set_weight(&next, key, kernel, basis, card.width, 0, 0.5).unwrap_err();
        assert!(error.contains("outside"), "{error}");
        assert!(set_weight(&next, key, kernel, basis, 0, 0, f32::NAN).is_err());
    }

    #[test]
    fn deactivating_a_cell_drops_it_from_the_active_count() {
        let spec = normalized();
        let key = binding(&spec);
        let kernel = binding_kernels(&spec, key, None)[0].id;
        // Activate a cell first, so the count being measured is one this test
        // put there rather than one the fixture happened to have.
        let painted = set_active(&spec, key, kernel, key.basis, 3, 3, true).unwrap();
        let painted = set_weight(&painted, key, kernel, key.basis, 3, 3, 0.5).unwrap();
        let before = binding_kernels(&painted, key, None)[0].support_cells;

        let masked = set_active(&painted, key, kernel, key.basis, 3, 3, false).unwrap();
        let after = binding_kernels(&masked, key, None)[0].support_cells;
        assert_eq!(after + 1, before, "an inactive cell must stop contributing");
    }

    #[test]
    fn a_weight_painted_on_an_inactive_cell_does_not_activate_it() {
        // Weight and support are separate: a cell the user has switched off
        // stays off, and the weight it holds waits until support returns.
        let spec = normalized();
        let key = binding(&spec);
        let kernel = binding_kernels(&spec, key, None)[0].id;
        let off = set_active(&spec, key, kernel, key.basis, 2, 2, false).unwrap();
        let before = binding_kernels(&off, key, None)[0].support_cells;
        let painted = set_weight(&off, key, kernel, key.basis, 2, 2, 0.9).unwrap();
        assert_eq!(
            binding_kernels(&painted, key, None)[0].support_cells,
            before,
            "painting must not silently switch a cell back on"
        );
        let on = set_active(&painted, key, kernel, key.basis, 2, 2, true).unwrap();
        assert_eq!(
            binding_kernels(&on, key, None)[0].support_cells,
            before + 1,
            "the weight was kept and counts again once support returns"
        );
    }
}
