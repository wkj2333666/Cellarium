//! Pure experiment transforms for the periodic tiling lifecycle.
//!
//! Closing a construction polygon is not just a geometry edit: a new basis has
//! to gain a weight plane in every kernel and a rule binding for every active
//! channel, or the draft it produces is one the validator rejects. That logic
//! was inlined in the terminal Workbench; it lives here so the Workbench and
//! the GUI commit identical drafts.

use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
use crate::sim::experiment_model::ExperimentSpec;
use crate::sim::ruleset::{KernelSpatialDefinition, RuleBinding};
use crate::sim::tiling::{
    BasisId, PeriodicTilingDraft, PrototypeId, PrototypeShape, RigidTransform, TileInstance,
    TilePrototype, TilingMode, TilingPreset, Vec2, build_preset, infer_translation_lattice,
    polygon, provisional_translation_lattice,
};

/// Reasons the experiment's tiling is not usable, in the user's words.
///
/// The Tiling workspace already computes this verdict and shows it in red. An
/// Apply that ignored it would be giving the user two authoritative and
/// opposite answers about the same draft — and the periodic kernels built from
/// a basis that leaves gaps do not describe the neighbourhood they claim to.
///
/// An experiment with no tiling has nothing to check; the raster world is the
/// geometry and it is always well formed.
pub fn coverage_problems(spec: &ExperimentSpec) -> Vec<String> {
    let Some(draft) = spec.tiling.as_ref() else {
        return Vec::new();
    };
    match crate::sim::tiling::validate_coverage(draft) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => {
            // One sentence naming the workspace that can fix it, then the
            // reasons themselves. A list of geometry facts with no route to the
            // control that changes them is not actionable.
            let mut problems = vec!["the tiling does not tile the plane".to_string()];
            problems.extend(diagnostics.into_iter().take(3).map(|entry| entry.message));
            problems
        }
    }
}

/// What a finished construction polygon becomes.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConstructionTarget {
    /// Append a new independent site to the unit cell.
    #[default]
    NewBasis,
    /// Reshape a polygon that already exists.
    ReplacePrototype(PrototypeId),
}

/// A committed construction: the draft to store and what the editor should
/// select so the new polygon is immediately editable.
#[derive(Clone, Debug, PartialEq)]
pub struct TilingCommit {
    pub spec: ExperimentSpec,
    pub prototype: PrototypeId,
    pub basis: Option<BasisId>,
}

/// Normalize a construction path into a closeable polygon, or say why it is not
/// one. Winding is corrected here so callers never have to care which way the
/// user happened to travel.
pub fn close_construction(construction: &[Vec2]) -> Result<Vec<Vec2>, String> {
    if construction.len() < 3 {
        return Err("place at least three vertices before closing the polygon".into());
    }
    let mut vertices = construction.to_vec();
    if polygon::signed_area(&vertices) < 0.0 {
        vertices.reverse();
    }
    if let Some(issue) = polygon::validate_polygon(&vertices).first() {
        return Err(issue.message.clone());
    }
    Ok(vertices)
}

/// Commit a construction polygon into the draft experiment.
pub fn finish_polygon(
    draft: &ExperimentSpec,
    construction: &[Vec2],
    target: ConstructionTarget,
) -> Result<TilingCommit, String> {
    let vertices = close_construction(construction)?;
    let mut next = draft.clone();
    match target {
        ConstructionTarget::NewBasis => {
            let (prototype, basis) = append_basis(&mut next, vertices)?;
            Ok(TilingCommit {
                spec: next,
                prototype,
                basis: Some(basis),
            })
        }
        ConstructionTarget::ReplacePrototype(selected) => {
            let prototype = next
                .tiling
                .as_mut()
                .and_then(|tiling| {
                    tiling
                        .prototypes
                        .iter_mut()
                        .find(|entry| entry.id == selected)
                })
                .ok_or("selected basis polygon is missing")?;
            prototype.shape = PrototypeShape::SimplePolygon { vertices };
            Ok(TilingCommit {
                spec: next,
                prototype: selected,
                basis: None,
            })
        }
    }
}

/// Add the polygon as a new basis and extend every dependent structure so the
/// resulting draft still validates.
fn append_basis(
    next: &mut ExperimentSpec,
    vertices: Vec<Vec2>,
) -> Result<(PrototypeId, BasisId), String> {
    if next.tiling.is_none() {
        // The first polygon has no lattice to belong to yet. Infer one from its
        // own edges, falling back to a bounding period that at least tiles.
        let (translation_a, translation_b) = infer_translation_lattice(&vertices)
            .unwrap_or_else(|_| provisional_translation_lattice(&vertices));
        next.tiling = Some(PeriodicTilingDraft {
            translation_a,
            translation_b,
            prototypes: Vec::new(),
            instances: Vec::new(),
            mode: TilingMode::Topological,
        });
    }
    let tiling = next
        .tiling
        .as_mut()
        .expect("new tiling was initialized above");
    let prototype = PrototypeId(
        tiling
            .prototypes
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("prototype id exhausted")?,
    );
    let basis = BasisId(
        tiling
            .instances
            .iter()
            .map(|entry| entry.id.0)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or("basis id exhausted")?,
    );
    tiling.prototypes.push(TilePrototype {
        id: prototype,
        name: format!("basis_{}", basis.0),
        shape: PrototypeShape::SimplePolygon { vertices },
    });
    tiling.instances.push(TileInstance {
        id: basis,
        prototype,
        transform: RigidTransform::default(),
    });

    if next.rules.is_empty() {
        *next = next.clone().normalize_rules().map_err(|errors| {
            errors
                .into_iter()
                .map(|error| error.to_string())
                .collect::<Vec<_>>()
                .join("; ")
        })?;
    }
    extend_kernels_with_basis(next, basis)?;
    bind_active_channels(next, basis)?;
    Ok((prototype, basis))
}

/// Give the new basis a weight plane in every kernel. A raster kernel is
/// promoted to a periodic one, since raster weights have no per-basis identity.
fn extend_kernels_with_basis(next: &mut ExperimentSpec, basis: BasisId) -> Result<(), String> {
    for rule in &mut next.rules.sets {
        for kernel in &mut rule.kernels {
            let replacement =
                match &mut kernel.spatial {
                    KernelSpatialDefinition::Raster(definition) => {
                        let built = definition.build().map_err(|error| error.to_string())?;
                        Some(KernelSpatialDefinition::Periodic(
                            PeriodicKernelDefinition {
                                width: built.width,
                                height: built.height,
                                anchor_x: built.anchor_x,
                                anchor_y: built.anchor_y,
                                planes: std::collections::BTreeMap::from([(
                                    basis,
                                    BasisWeightPlane {
                                        values: built.values,
                                        mask: built.mask,
                                    },
                                )]),
                            },
                        ))
                    }
                    KernelSpatialDefinition::Periodic(definition) => {
                        let plane_len = definition.width * definition.height;
                        let template = definition.planes.values().next().cloned().unwrap_or(
                            BasisWeightPlane {
                                values: vec![0.0; plane_len],
                                mask: None,
                            },
                        );
                        definition.planes.insert(basis, template);
                        None
                    }
                };
            if let Some(replacement) = replacement {
                kernel.spatial = replacement;
            }
        }
    }
    Ok(())
}

/// Bind the new basis to the default rule-set of every channel that is still
/// being updated. A frozen channel is deliberately left unbound.
fn bind_active_channels(next: &mut ExperimentSpec, basis: BasisId) -> Result<(), String> {
    for output in next
        .channels
        .iter()
        .filter(|channel| !channel.frozen)
        .map(|channel| channel.id)
        .collect::<Vec<_>>()
    {
        let default = *next
            .rules
            .defaults
            .get(&output)
            .ok_or("active channel has no default rule-set")?;
        if next.rules.binding(basis, output).is_none() {
            next.rules.bindings.push(RuleBinding {
                basis,
                output,
                rule_set: default,
            });
        }
    }
    Ok(())
}

/// Replace the whole tiling with a preset unit cell.
pub fn apply_preset(
    draft: &ExperimentSpec,
    preset: TilingPreset,
    scale: f64,
) -> Result<ExperimentSpec, String> {
    let built = build_preset(preset, scale);
    let mut next = draft.clone();
    next.tiling = None;
    let mut prototypes = Vec::new();
    // Committing the preset one polygon at a time reuses the invariant-keeping
    // path above, so a preset draft is exactly as valid as a drawn one.
    for instance in &built.instances {
        let prototype = built
            .prototypes
            .iter()
            .find(|entry| entry.id == instance.prototype)
            .ok_or("preset instance references a missing prototype")?;
        let vertices = polygon::prototype_vertices(&prototype.shape).map_err(|issues| {
            issues
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>()
                .join("; ")
        })?;
        let placed = polygon::transform_vertices(&vertices, instance.transform);
        let commit = finish_polygon(&next, &placed, ConstructionTarget::NewBasis)?;
        next = commit.spec;
        prototypes.push(commit.prototype);
    }
    // The preset's own lattice is authoritative; inference only served the
    // first polygon, which knew nothing about the ones that follow.
    if let Some(tiling) = next.tiling.as_mut() {
        tiling.translation_a = built.translation_a;
        tiling.translation_b = built.translation_b;
        tiling.mode = built.mode;
        for (prototype, source) in prototypes.iter().zip(&built.prototypes) {
            if let Some(entry) = tiling
                .prototypes
                .iter_mut()
                .find(|entry| entry.id == *prototype)
            {
                entry.name = source.name.clone();
            }
        }
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank() -> ExperimentSpec {
        let mut spec = ExperimentSpec::single_channel_lenia(16, 16);
        spec.tiling = None;
        spec
    }

    fn triangle() -> Vec<Vec2> {
        vec![
            Vec2::new(-0.5, -0.4),
            Vec2::new(0.5, -0.4),
            Vec2::new(0.0, 0.5),
        ]
    }

    #[test]
    fn a_path_shorter_than_a_triangle_cannot_close() {
        let error = close_construction(&triangle()[..2]).unwrap_err();
        assert!(error.contains("three vertices"), "{error}");
    }

    #[test]
    fn closing_corrects_the_winding_the_user_happened_to_draw() {
        let mut reversed = triangle();
        reversed.reverse();
        let forward = close_construction(&triangle()).unwrap();
        let corrected = close_construction(&reversed).unwrap();
        assert!(polygon::signed_area(&forward) > 0.0);
        assert!(polygon::signed_area(&corrected) > 0.0);
    }

    #[test]
    fn the_first_polygon_creates_a_lattice_and_a_bound_basis() {
        let commit = finish_polygon(&blank(), &triangle(), ConstructionTarget::NewBasis).unwrap();
        let tiling = commit.spec.tiling.as_ref().unwrap();
        assert_eq!(tiling.instances.len(), 1);
        assert!(tiling.translation_a.length() > 0.0);
        let basis = commit.basis.unwrap();
        for channel in commit.spec.channels.iter().filter(|entry| !entry.frozen) {
            assert!(
                commit.spec.rules.binding(basis, channel.id).is_some(),
                "an active channel must be bound on the new basis"
            );
        }
    }

    #[test]
    fn a_second_basis_gains_a_weight_plane_in_every_kernel() {
        let first = finish_polygon(&blank(), &triangle(), ConstructionTarget::NewBasis).unwrap();
        let shifted: Vec<Vec2> = triangle()
            .iter()
            .map(|point| *point + Vec2::new(4.0, 0.0))
            .collect();
        let second = finish_polygon(&first.spec, &shifted, ConstructionTarget::NewBasis).unwrap();
        let basis = second.basis.unwrap();
        for rule in &second.spec.rules.sets {
            for kernel in &rule.kernels {
                match &kernel.spatial {
                    KernelSpatialDefinition::Periodic(definition) => {
                        assert!(
                            definition.planes.contains_key(&basis),
                            "every kernel must cover the new basis"
                        );
                    }
                    KernelSpatialDefinition::Raster(_) => {
                        panic!("a basis-aware draft must not keep raster kernels")
                    }
                }
            }
        }
    }

    #[test]
    fn replacing_a_prototype_reshapes_it_without_adding_a_basis() {
        let first = finish_polygon(&blank(), &triangle(), ConstructionTarget::NewBasis).unwrap();
        let wider = vec![
            Vec2::new(-1.0, -0.4),
            Vec2::new(1.0, -0.4),
            Vec2::new(0.0, 0.5),
        ];
        let second = finish_polygon(
            &first.spec,
            &wider,
            ConstructionTarget::ReplacePrototype(first.prototype),
        )
        .unwrap();
        let tiling = second.spec.tiling.as_ref().unwrap();
        assert_eq!(tiling.instances.len(), 1);
        assert_eq!(second.basis, None);
        let PrototypeShape::SimplePolygon { vertices } = &tiling.prototypes[0].shape else {
            panic!("a drawn polygon stays a simple polygon");
        };
        assert_eq!(vertices.len(), 3);
    }

    #[test]
    fn every_preset_commits_a_draft_with_its_own_lattice() {
        for preset in TilingPreset::ALL {
            let spec = apply_preset(&blank(), preset, 1.0).unwrap();
            let tiling = spec.tiling.as_ref().unwrap();
            let expected = build_preset(preset, 1.0);
            assert_eq!(
                tiling.instances.len(),
                expected.instances.len(),
                "{preset:?}"
            );
            assert_eq!(tiling.translation_a, expected.translation_a, "{preset:?}");
            assert_eq!(tiling.translation_b, expected.translation_b, "{preset:?}");
        }
    }
}
