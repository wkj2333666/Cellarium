use serde::{Deserialize, Serialize};

use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
use crate::sim::tiling::{
    BasisId, PeriodicTilingDraft, Vec2,
    polygon::{prototype_vertices, transform_vertices},
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum KernelSamplingMetric {
    LatticeAffine,
    WorldEuclidean,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub enum KernelProfile {
    Gaussian { sigma: f64 },
    Ring { radius: f64, width: f64 },
    Constant,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct KernelGenerationSpec {
    pub metric: KernelSamplingMetric,
    pub profile: KernelProfile,
    pub amplitude: f64,
    pub support_radius: Option<f64>,
}

pub fn generate_periodic_plane(
    tiling: &PeriodicTilingDraft,
    target_basis: BasisId,
    source_basis: BasisId,
    definition: &PeriodicKernelDefinition,
    spec: &KernelGenerationSpec,
) -> Result<BasisWeightPlane, String> {
    definition.validate().map_err(|error| error.to_string())?;
    validate_generation_spec(spec)?;
    let target_site = basis_site(tiling, target_basis)?;
    let source_site = basis_site(tiling, source_basis)?;
    let existing_plane = definition
        .planes
        .get(&source_basis)
        .ok_or_else(|| format!("kernel has no source-basis plane {}", source_basis.0))?;
    let mut values = Vec::with_capacity(definition.width * definition.height);
    let mut mask = Vec::with_capacity(definition.width * definition.height);

    for y in 0..definition.height {
        for x in 0..definition.width {
            let offset = [
                x as i16 - definition.anchor_x as i16,
                y as i16 - definition.anchor_y as i16,
            ];
            let distance = match spec.metric {
                KernelSamplingMetric::LatticeAffine => {
                    f64::from(offset[0]).hypot(f64::from(offset[1]))
                }
                KernelSamplingMetric::WorldEuclidean => (tiling.translation_a
                    * f64::from(offset[0])
                    + tiling.translation_b * f64::from(offset[1])
                    + source_site
                    - target_site)
                    .length(),
            };
            let index = y * definition.width + x;
            let active = spec.support_radius.map_or_else(
                || {
                    existing_plane
                        .mask
                        .as_ref()
                        .is_none_or(|existing_mask| existing_mask[index])
                },
                |support_radius| distance <= support_radius,
            );
            mask.push(active);
            let value = if active {
                spec.amplitude * sample_profile(spec.profile, distance)
            } else {
                0.0
            };
            if !value.is_finite() || value < f32::MIN as f64 || value > f32::MAX as f64 {
                return Err(format!(
                    "kernel generator produced a non-finite weight at offset {offset:?}"
                ));
            }
            values.push(value as f32);
        }
    }

    Ok(BasisWeightPlane {
        values,
        mask: Some(mask),
    })
}

fn validate_generation_spec(spec: &KernelGenerationSpec) -> Result<(), String> {
    if !spec.amplitude.is_finite() {
        return Err("kernel amplitude must be finite".into());
    }
    if spec
        .support_radius
        .is_some_and(|radius| !radius.is_finite() || radius < 0.0)
    {
        return Err("kernel support radius must be finite and non-negative".into());
    }
    match spec.profile {
        KernelProfile::Gaussian { sigma } if !sigma.is_finite() || sigma <= 0.0 => {
            Err("Gaussian sigma must be finite and positive".into())
        }
        KernelProfile::Ring { radius, width }
            if !radius.is_finite() || radius < 0.0 || !width.is_finite() || width <= 0.0 =>
        {
            Err("ring radius must be non-negative and width must be positive".into())
        }
        _ => Ok(()),
    }
}

fn sample_profile(profile: KernelProfile, distance: f64) -> f64 {
    match profile {
        KernelProfile::Gaussian { sigma } => (-0.5 * distance * distance / (sigma * sigma)).exp(),
        KernelProfile::Ring { radius, width } => {
            let normalized = (distance - radius) / width;
            (-normalized * normalized).exp()
        }
        KernelProfile::Constant => 1.0,
    }
}

fn basis_site(tiling: &PeriodicTilingDraft, basis: BasisId) -> Result<Vec2, String> {
    let instance = tiling
        .instances
        .iter()
        .find(|instance| instance.id == basis)
        .ok_or_else(|| format!("tiling has no basis {}", basis.0))?;
    let prototype = tiling
        .prototypes
        .iter()
        .find(|prototype| prototype.id == instance.prototype)
        .ok_or_else(|| format!("basis {} references a missing prototype", basis.0))?;
    let vertices = prototype_vertices(&prototype.shape)
        .map_err(|issues| {
            issues
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .map(|vertices| transform_vertices(&vertices, instance.transform))?;
    polygon_area_centroid(&vertices)
}

fn polygon_area_centroid(vertices: &[Vec2]) -> Result<Vec2, String> {
    if vertices.len() < 3 {
        return Err("basis polygon needs at least three vertices".into());
    }
    let mut cross_sum = 0.0;
    let mut weighted = Vec2::ZERO;
    for index in 0..vertices.len() {
        let a = vertices[index];
        let b = vertices[(index + 1) % vertices.len()];
        let cross = a.cross(b);
        cross_sum += cross;
        weighted = weighted + (a + b) * cross;
    }
    if !cross_sum.is_finite() || cross_sum.abs() <= 1.0e-14 {
        return Err("basis polygon has no finite area centroid".into());
    }
    Ok(weighted * (1.0 / (3.0 * cross_sum)))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::sim::basis_kernel::{BasisWeightPlane, PeriodicKernelDefinition};
    use crate::sim::tiling::{BasisId, TilingPreset, build_preset};

    fn three_by_three() -> PeriodicKernelDefinition {
        PeriodicKernelDefinition {
            width: 3,
            height: 3,
            anchor_x: 1,
            anchor_y: 1,
            planes: BTreeMap::from([(
                BasisId(0),
                BasisWeightPlane {
                    values: vec![0.0; 9],
                    mask: Some(vec![true; 9]),
                },
            )]),
        }
    }

    fn value(plane: &BasisWeightPlane, offset: [i16; 2]) -> f32 {
        let x = usize::try_from(1_i16 + offset[0]).unwrap();
        let y = usize::try_from(1_i16 + offset[1]).unwrap();
        plane.values[y * 3 + x]
    }

    #[test]
    fn world_gaussian_gives_six_hex_neighbors_equal_weight() {
        let tiling = build_preset(TilingPreset::RegularHexagon, 1.0);
        let definition = three_by_three();
        let plane = generate_periodic_plane(
            &tiling,
            BasisId(0),
            BasisId(0),
            &definition,
            &KernelGenerationSpec {
                metric: KernelSamplingMetric::WorldEuclidean,
                profile: KernelProfile::Gaussian { sigma: 1.0 },
                amplitude: 1.0,
                support_radius: None,
            },
        )
        .unwrap();
        let weights = [
            value(&plane, [1, 0]),
            value(&plane, [0, 1]),
            value(&plane, [-1, 1]),
            value(&plane, [-1, 0]),
            value(&plane, [0, -1]),
            value(&plane, [1, -1]),
        ];

        assert!(
            weights
                .windows(2)
                .all(|pair| (pair[0] - pair[1]).abs() < 1.0e-6),
            "world-space nearest-neighbor weights were {weights:?}",
        );
    }

    #[test]
    fn affine_and_world_metrics_differ_on_an_oblique_hex_lattice() {
        let tiling = build_preset(TilingPreset::RegularHexagon, 1.0);
        let definition = three_by_three();
        let generate = |metric| {
            generate_periodic_plane(
                &tiling,
                BasisId(0),
                BasisId(0),
                &definition,
                &KernelGenerationSpec {
                    metric,
                    profile: KernelProfile::Gaussian { sigma: 1.0 },
                    amplitude: 1.0,
                    support_radius: None,
                },
            )
            .unwrap()
        };
        let affine = generate(KernelSamplingMetric::LatticeAffine);
        let world = generate(KernelSamplingMetric::WorldEuclidean);

        assert!((value(&affine, [1, 0]) - value(&affine, [1, -1])).abs() > 0.1);
        assert!((value(&world, [1, 0]) - value(&world, [1, -1])).abs() < 1.0e-6);
    }

    #[test]
    fn value_preset_without_support_radius_preserves_the_existing_mask() {
        let tiling = build_preset(TilingPreset::Square, 1.0);
        let mut definition = three_by_three();
        let plane = definition.planes.get_mut(&BasisId(0)).unwrap();
        plane.mask = Some(vec![false, true, true, true, true, true, true, true, true]);
        let generated = generate_periodic_plane(
            &tiling,
            BasisId(0),
            BasisId(0),
            &definition,
            &KernelGenerationSpec {
                metric: KernelSamplingMetric::LatticeAffine,
                profile: KernelProfile::Constant,
                amplitude: 1.0,
                support_radius: None,
            },
        )
        .unwrap();

        assert_eq!(generated.mask.as_ref().unwrap()[0], false);
        assert_eq!(generated.values[0], 0.0);
        assert!(generated.values[1..].iter().all(|value| *value == 1.0));
    }
}
