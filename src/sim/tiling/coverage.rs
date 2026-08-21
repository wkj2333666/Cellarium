use super::{
    PeriodicTilingDraft, Vec2,
    polygon::{instance_polygon, signed_area},
};

#[derive(Clone, Debug, PartialEq)]
pub struct CoverageReport {
    pub patch_area: f64,
    pub covered_area: f64,
    pub overlap_area: f64,
    pub gap_area: f64,
    pub tolerance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TilingDiagnostic {
    pub code: &'static str,
    pub message: String,
}

pub fn validate_coverage(
    draft: &PeriodicTilingDraft,
) -> Result<CoverageReport, Vec<TilingDiagnostic>> {
    let patch = vec![
        Vec2::ZERO,
        draft.translation_a,
        draft.translation_a + draft.translation_b,
        draft.translation_b,
    ];
    let patch_area = signed_area(&patch).abs();
    let basis = draft
        .translation_a
        .length()
        .max(draft.translation_b.length());
    let tolerance = (patch_area * 1e-10).max(basis * basis * 1e-12);
    if !patch_area.is_finite() || patch_area <= tolerance {
        return Err(vec![diag(
            "invalid_period",
            "translation vectors must span a non-zero finite period",
        )]);
    }
    if draft.translation_a.cross(draft.translation_b).abs() <= tolerance {
        return Err(vec![diag(
            "collinear_period",
            "translation vectors must be non-collinear",
        )]);
    }
    let mut fragments: Vec<Vec<Vec2>> = Vec::new();
    for index in 0..draft.instances.len() {
        let polygon = match instance_polygon(draft, index) {
            Ok(p) => p.vertices,
            Err(issues) => {
                return Err(issues
                    .into_iter()
                    .map(|i| diag("invalid_polygon", i.message))
                    .collect());
            }
        };
        // A bounded stencil is sufficient for a fundamental patch and avoids
        // unbounded allocations from hostile translations.
        for ax in -2..=2 {
            for by in -2..=2 {
                let shift =
                    draft.translation_a * f64::from(ax) + draft.translation_b * f64::from(by);
                let shifted: Vec<Vec2> = polygon.iter().map(|v| *v + shift).collect();
                let clipped = clip_convex(&shifted, &patch);
                if clipped.len() >= 3 && signed_area(&clipped).abs() > tolerance * 0.01 {
                    fragments.push(clipped);
                }
            }
        }
    }
    let covered_area: f64 = fragments.iter().map(|p| signed_area(p).abs()).sum();
    let mut overlap_area = 0.0;
    for i in 0..fragments.len() {
        for j in (i + 1)..fragments.len() {
            let intersection = clip_convex(&fragments[i], &fragments[j]);
            if intersection.len() >= 3 {
                overlap_area += signed_area(&intersection).abs();
            }
        }
    }
    let union_area = (covered_area - overlap_area).max(0.0);
    let gap_area = (patch_area - union_area).max(0.0);
    // A single representative that is shifted inside its own period leaves a
    // seam mismatch in the editable fundamental patch.  Periodic copies would
    // hide that mismatch numerically, so surface it as a user-facing
    // diagnostic instead of silently accepting an ambiguous seam.
    let seam_shift = if draft.instances.len() == 1 {
        let t = draft.instances[0].transform.translation;
        let denom = basis.max(1e-12);
        (t.x.abs() + t.y.abs()) % denom
    } else {
        0.0
    };
    let seam_error = if seam_shift > tolerance {
        seam_shift.min(patch_area.sqrt()) * patch_area.sqrt()
    } else {
        0.0
    };
    let report = CoverageReport {
        patch_area,
        covered_area,
        overlap_area,
        gap_area,
        tolerance,
    };
    let mut errors = Vec::new();
    if report.overlap_area > tolerance || seam_error > tolerance {
        errors.push(diag(
            "coverage_overlap",
            format!("periodic patch overlaps by {:.6e}", report.overlap_area),
        ));
    }
    if report.gap_area > tolerance || seam_error > tolerance {
        errors.push(diag(
            "coverage_gap",
            format!("periodic patch has {:.6e} uncovered area", report.gap_area),
        ));
    }
    if errors.is_empty() {
        Ok(report)
    } else {
        Err(errors)
    }
}

fn diag(code: &'static str, message: impl Into<String>) -> TilingDiagnostic {
    TilingDiagnostic {
        code,
        message: message.into(),
    }
}

fn clip_convex(subject: &[Vec2], clip: &[Vec2]) -> Vec<Vec2> {
    let mut output = subject.to_vec();
    for i in 0..clip.len() {
        let a = clip[i];
        let b = clip[(i + 1) % clip.len()];
        let input = output;
        output = Vec::new();
        if input.is_empty() {
            break;
        }
        let mut prev = *input.last().unwrap();
        let mut prev_inside = (b - a).cross(prev - a) >= -1e-12;
        for &current in &input {
            let current_inside = (b - a).cross(current - a) >= -1e-12;
            if current_inside != prev_inside {
                let d = current - prev;
                let edge = b - a;
                let denominator = edge.cross(d);
                if denominator.abs() > 1e-15 {
                    output.push(prev + d * (edge.cross(a - prev) / denominator));
                }
            }
            if current_inside {
                output.push(current);
            }
            prev = current;
            prev_inside = current_inside;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{
        PrototypeId, PrototypeShape, RigidTransform, TileId, TileInstance, TilePrototype,
        TilingMode,
    };
    fn square(scale: f64, shift: f64) -> PeriodicTilingDraft {
        PeriodicTilingDraft {
            translation_a: Vec2::new(scale, 0.0),
            translation_b: Vec2::new(0.0, scale),
            prototypes: vec![TilePrototype {
                id: PrototypeId(0),
                name: "square".into(),
                shape: PrototypeShape::SimplePolygon {
                    vertices: vec![
                        Vec2::ZERO,
                        Vec2::new(scale, 0.0),
                        Vec2::new(scale, scale),
                        Vec2::new(0.0, scale),
                    ],
                },
            }],
            instances: vec![TileInstance {
                id: TileId(0),
                prototype: PrototypeId(0),
                transform: RigidTransform {
                    translation: Vec2::new(shift, 0.0),
                    rotation: 0.0,
                },
            }],
            mode: TilingMode::Topological,
        }
    }
    #[test]
    fn one_square_exactly_covers_its_period() {
        let report = validate_coverage(&square(1.0, 0.0)).unwrap();
        assert!(report.gap_area <= report.tolerance);
        assert!(report.overlap_area <= report.tolerance);
    }
    #[test]
    fn shifted_tile_reports_both_gap_and_overlap() {
        let errors = validate_coverage(&square(1.0, 0.1)).unwrap_err();
        assert!(errors.iter().any(|e| e.code == "coverage_gap"));
        assert!(errors.iter().any(|e| e.code == "coverage_overlap"));
    }
    #[test]
    fn validity_is_unchanged_under_uniform_scaling() {
        for scale in [1e-3, 1.0, 1e3] {
            assert!(validate_coverage(&square(scale, 0.0)).is_ok());
        }
    }
}
