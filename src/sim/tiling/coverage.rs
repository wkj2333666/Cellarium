use std::collections::BTreeMap;

use super::{
    BasisId, GeometryBudget, NeighborPlacement, PeriodicArrangement, PeriodicTilingDraft,
    polygon::instance_polygon,
};

#[derive(Clone, Debug, PartialEq)]
pub struct TilingValidationReport {
    pub coverage_multiplicity: u32,
    pub face_area: f64,
    pub patch_area: f64,
    pub covered_area: f64,
    pub overlap_area: f64,
    pub gap_area: f64,
    pub tolerance: f64,
    pub euler_characteristic: i64,
    pub atomic_edges: usize,
    pub neighbor_ring: BTreeMap<BasisId, Vec<NeighborPlacement>>,
    pub arrangement: PeriodicArrangement,
}

pub type CoverageReport = TilingValidationReport;

#[derive(Clone, Debug, PartialEq)]
pub struct TilingDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub path: Option<String>,
}

pub fn validate_coverage(
    draft: &PeriodicTilingDraft,
) -> Result<CoverageReport, Vec<TilingDiagnostic>> {
    validate_periodic_tiling(draft, GeometryBudget::authoritative())
}

pub fn validate_periodic_tiling(
    draft: &PeriodicTilingDraft,
    budget: GeometryBudget,
) -> Result<TilingValidationReport, Vec<TilingDiagnostic>> {
    let patch_area = draft.translation_a.cross(draft.translation_b).abs();
    let lattice_scale = draft
        .translation_a
        .length()
        .max(draft.translation_b.length());
    let tolerance = (patch_area * 1e-9)
        .max(lattice_scale * lattice_scale * f64::EPSILON * 64.0)
        .max(f64::MIN_POSITIVE);
    if !patch_area.is_finite() || patch_area <= tolerance {
        return Err(vec![diagnostic(
            "invalid_period",
            "translation vectors must span a non-zero finite period",
        )]);
    }

    let mut diagnostics = Vec::new();
    let mut raw_face_area = 0.0;
    for (index, instance) in draft.instances.iter().enumerate() {
        match instance_polygon(draft, index) {
            Ok(polygon) => raw_face_area += polygon.unsigned_area(),
            Err(issues) => diagnostics.extend(issues.into_iter().map(|issue| TilingDiagnostic {
                code: issue.code,
                message: issue.message,
                path: Some(format!("basis/{}", instance.id.0)),
            })),
        }
    }
    // Stated as a share of the unit cell. "covers 78% of the unit cell, so 22%
    // is left bare" is something a user can act on; two areas in scientific
    // notation is something they have to do arithmetic on first.
    if raw_face_area < patch_area - tolerance {
        let share = raw_face_area / patch_area;
        diagnostics.push(diagnostic(
            "coverage_gap",
            format!(
                "the tiles cover {:.0}% of the unit cell, leaving {:.0}% bare",
                share * 100.0,
                (1.0 - share) * 100.0
            ),
        ));
    } else if raw_face_area > patch_area + tolerance {
        let share = raw_face_area / patch_area;
        diagnostics.push(diagnostic(
            "coverage_overlap",
            format!(
                "the tiles cover {:.0}% of the unit cell, so they overlap by {:.0}%",
                share * 100.0,
                (share - 1.0) * 100.0
            ),
        ));
    }

    let arrangement = match PeriodicArrangement::build(draft, budget) {
        Ok(arrangement) => Some(arrangement),
        Err(errors) => {
            diagnostics.extend(errors);
            None
        }
    };
    if !diagnostics.is_empty() {
        stable_diagnostics(&mut diagnostics);
        return Err(diagnostics);
    }
    let arrangement = arrangement.expect("successful arrangement is present");
    let face_area = arrangement
        .faces
        .iter()
        .map(|face| face.signed_area)
        .sum::<f64>();
    if (face_area - patch_area).abs() > tolerance {
        return Err(vec![diagnostic(
            if face_area < patch_area {
                "coverage_gap"
            } else {
                "coverage_overlap"
            },
            format!(
                "oriented arrangement area {face_area:.12e} differs from patch area {patch_area:.12e}"
            ),
        )]);
    }

    // Unique opposite atomic twins and no proper crossings make the winding
    // number constant on the connected torus. The oriented area ratio fixes
    // that integer constant to one, proving exact-once coverage without
    // pairwise inclusion/exclusion.
    let multiplicity = (face_area / patch_area).round();
    if multiplicity != 1.0 {
        return Err(vec![diagnostic(
            "coverage_multiplicity",
            format!("periodic coverage multiplicity is {multiplicity}"),
        )]);
    }
    let vertices = i64::try_from(arrangement.vertices.len()).unwrap_or(i64::MAX);
    let undirected_edges = i64::try_from(arrangement.atomic_edges.len() / 2).unwrap_or(i64::MAX);
    let faces = i64::try_from(arrangement.faces.len()).unwrap_or(i64::MAX);
    let euler_characteristic = vertices - undirected_edges + faces;
    if euler_characteristic != 0 {
        return Err(vec![diagnostic(
            "invalid_euler_characteristic",
            format!("toroidal arrangement has V-E+F={euler_characteristic}; expected 0"),
        )]);
    }
    let neighbor_ring = arrangement
        .faces
        .iter()
        .map(|face| (face.basis, arrangement.neighbor_ring(face.basis)))
        .collect();
    Ok(TilingValidationReport {
        coverage_multiplicity: 1,
        face_area,
        patch_area,
        covered_area: face_area,
        overlap_area: 0.0,
        gap_area: 0.0,
        tolerance,
        euler_characteristic,
        atomic_edges: arrangement.atomic_edges.len(),
        neighbor_ring,
        arrangement,
    })
}

/// Lattice offsets of the periodic copies that actually touch the unit cell.
///
/// The ring comes from the validated arrangement rather than a blanket 3x3
/// sweep, so an oblique lattice shows the neighbours it really has.
pub fn neighbor_offsets(draft: &PeriodicTilingDraft) -> Vec<[i32; 2]> {
    let Ok(report) = validate_coverage(draft) else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    report
        .neighbor_ring
        .values()
        .flatten()
        .map(|neighbor| neighbor.lattice_offset)
        .filter(|offset| *offset != [0, 0])
        .filter(|offset| seen.insert(*offset))
        .collect()
}

fn stable_diagnostics(diagnostics: &mut Vec<TilingDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        left.code
            .cmp(right.code)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.message.cmp(&right.message))
    });
    diagnostics.dedup();
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> TilingDiagnostic {
    TilingDiagnostic {
        code,
        message: message.into(),
        path: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{
        PrototypeId, PrototypeShape, RigidTransform, TileId, TileInstance, TilePrototype,
        TilingMode, TilingPreset, Vec2, build_preset,
    };

    fn polygon(id: u32, vertices: &[[f64; 2]]) -> TilePrototype {
        TilePrototype {
            id: PrototypeId(id),
            name: format!("polygon-{id}"),
            shape: PrototypeShape::SimplePolygon {
                vertices: vertices
                    .iter()
                    .map(|point| Vec2::new(point[0], point[1]))
                    .collect(),
            },
        }
    }

    fn instance(id: u32) -> TileInstance {
        TileInstance {
            id: TileId(id),
            prototype: PrototypeId(id),
            transform: RigidTransform::default(),
        }
    }

    #[test]
    fn exact_once_fixtures_have_unit_multiplicity_and_torus_euler_zero() {
        for draft in [
            build_preset(TilingPreset::Square, 1.0),
            build_preset(TilingPreset::EquilateralTriangles, 1.0),
            build_preset(TilingPreset::RegularHexagon, 1.0),
            build_preset(TilingPreset::OctagonSquare, 1.0),
        ] {
            let report = validate_periodic_tiling(&draft, GeometryBudget::authoritative()).unwrap();
            assert_eq!(report.coverage_multiplicity, 1);
            assert_eq!(report.euler_characteristic, 0);
            assert!((report.face_area - report.patch_area).abs() <= report.tolerance);
        }
    }

    #[test]
    fn translating_the_representative_inside_its_period_remains_a_valid_tiling() {
        let mut square = build_preset(TilingPreset::Square, 1.0);
        square.instances[0].transform.translation = Vec2::new(0.1, 0.35);
        assert!(validate_coverage(&square).is_ok());
    }

    #[test]
    fn gap_overlap_duplicate_and_crossing_fixtures_fail() {
        let gap = PeriodicTilingDraft {
            translation_a: Vec2::new(2.0, 0.0),
            translation_b: Vec2::new(0.0, 2.0),
            prototypes: vec![polygon(
                0,
                &[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            )],
            instances: vec![instance(0)],
            mode: TilingMode::Topological,
        };
        let gap_errors = validate_coverage(&gap).unwrap_err();
        assert!(gap_errors.iter().any(|error| error.code == "coverage_gap"));

        let mut overlap = build_preset(TilingPreset::Square, 1.0);
        overlap.instances.push(TileInstance {
            id: TileId(1),
            prototype: PrototypeId(0),
            transform: RigidTransform::default(),
        });
        let overlap_errors = validate_coverage(&overlap).unwrap_err();
        assert!(
            overlap_errors
                .iter()
                .any(|error| error.code == "coverage_overlap")
        );
        assert!(
            overlap_errors
                .iter()
                .any(|error| error.code == "competing_twins")
        );

        overlap.instances.push(TileInstance {
            id: TileId(2),
            prototype: PrototypeId(0),
            transform: RigidTransform::default(),
        });
        let triple_errors = validate_coverage(&overlap).unwrap_err();
        assert!(
            triple_errors
                .iter()
                .any(|error| error.code == "coverage_overlap")
        );

        let mut crossing = build_preset(TilingPreset::Square, 2.0);
        crossing.prototypes.push(polygon(
            1,
            &[[0.5, -0.5], [2.5, 0.5], [1.5, 2.5], [-0.5, 1.5]],
        ));
        crossing.instances.push(instance(1));
        assert!(
            validate_coverage(&crossing)
                .unwrap_err()
                .iter()
                .any(|error| error.code == "proper_crossing")
        );
    }

    #[test]
    fn uniform_scaling_does_not_change_validity() {
        for scale in [1e-6, 1.0, 1e6] {
            assert!(validate_coverage(&build_preset(TilingPreset::Square, scale)).is_ok());
        }
    }

    #[test]
    fn persisted_valid_and_invalid_fixtures_match_the_validator() {
        const VALID: &[(&str, &str)] = &[
            (
                "square",
                include_str!("../../../tests/fixtures/tiling/square.ron"),
            ),
            (
                "triangles",
                include_str!("../../../tests/fixtures/tiling/triangles.ron"),
            ),
            (
                "regular hexagon",
                include_str!("../../../tests/fixtures/tiling/regular_hexagon.ron"),
            ),
            (
                "honeycomb",
                include_str!("../../../tests/fixtures/tiling/honeycomb.ron"),
            ),
            (
                "octagon-square",
                include_str!("../../../tests/fixtures/tiling/octagon_square.ron"),
            ),
        ];
        for (name, source) in VALID {
            let draft: PeriodicTilingDraft = ron::from_str(source).unwrap();
            let report =
                validate_coverage(&draft).unwrap_or_else(|errors| panic!("{name}: {errors:?}"));
            assert_eq!(report.coverage_multiplicity, 1, "{name}");
            assert_eq!(report.euler_characteristic, 0, "{name}");
        }

        const INVALID: &[(&str, &str)] = &[
            (
                "gap",
                include_str!("../../../tests/fixtures/tiling/invalid_gap.ron"),
            ),
            (
                "overlap",
                include_str!("../../../tests/fixtures/tiling/invalid_overlap.ron"),
            ),
            (
                "crossing",
                include_str!("../../../tests/fixtures/tiling/invalid_crossing.ron"),
            ),
        ];
        for (name, source) in INVALID {
            let draft: PeriodicTilingDraft = ron::from_str(source).unwrap();
            assert!(validate_coverage(&draft).is_err(), "{name}");
        }
        let t_junction: PeriodicTilingDraft = ron::from_str(include_str!(
            "../../../tests/fixtures/tiling/t_junction.ron"
        ))
        .unwrap();
        let errors = validate_coverage(&t_junction).unwrap_err();
        assert!(errors.iter().any(|error| error.code == "t_junction"));
    }
}
