//! Inferring a translation lattice from one polygon.
//!
//! A polygon drawn from scratch has no lattice yet. When its edges pair up into
//! opposite parallel halves it determines one exactly, and that pair is what
//! tiles the plane. When they do not, a provisional period from the bounding
//! box at least lets the copies be drawn while the user keeps working.

use super::model::{PeriodicTilingDraft, PrototypeId, PrototypeShape, Vec2};
use super::polygon::validate_polygon;

/// Infer the two translation vectors a polygon's own edges determine.
///
/// Only a polygon whose edges cancel in opposite pairs has an exact answer, so
/// a triangle is refused rather than given a lattice that does not tile: the
pub fn infer_translation_lattice(vertices: &[Vec2]) -> Result<(Vec2, Vec2), String> {
    if let Some(issue) = validate_polygon(vertices).into_iter().next() {
        return Err(issue.message);
    }
    let scale = vertices
        .iter()
        .zip(vertices.iter().cycle().skip(1))
        .take(vertices.len())
        .map(|(start, end)| (*end - *start).length())
        .fold(0.0_f64, f64::max)
        .max(1.0);
    let tolerance = scale * 1e-8;
    let mut candidates = Vec::new();
    for first in 0..vertices.len() {
        let first_edge = vertices[(first + 1) % vertices.len()] - vertices[first];
        for second in first + 1..vertices.len() {
            let second_edge = vertices[(second + 1) % vertices.len()] - vertices[second];
            if (first_edge + second_edge).length() > tolerance {
                continue;
            }
            let translation = vertices[first] - vertices[(second + 1) % vertices.len()];
            if translation.length() <= tolerance {
                continue;
            }
            for candidate in [translation, translation * -1.0] {
                if !candidates
                    .iter()
                    .any(|existing: &Vec2| (*existing - candidate).length() <= tolerance)
                {
                    candidates.push(candidate);
                }
            }
        }
    }
    let area = vertices
        .iter()
        .zip(vertices.iter().cycle().skip(1))
        .take(vertices.len())
        .map(|(start, end)| start.cross(*end))
        .sum::<f64>()
        .abs()
        * 0.5;
    let area_tolerance = (area * 1e-7).max(tolerance * tolerance);
    let mut validation_attempts = 0usize;
    for left in 0..candidates.len() {
        for right in left + 1..candidates.len() {
            let mut a = candidates[left];
            let mut b = candidates[right];
            let determinant = a.cross(b);
            if determinant.abs() <= area_tolerance
                || (determinant.abs() - area).abs() > area_tolerance
            {
                continue;
            }
            validation_attempts += 1;
            if validation_attempts > 128 {
                return Err("lattice inference exceeded its candidate budget".into());
            }
            if determinant < 0.0 {
                std::mem::swap(&mut a, &mut b);
            }
            let draft = PeriodicTilingDraft {
                translation_a: a,
                translation_b: b,
                prototypes: vec![crate::sim::tiling::TilePrototype {
                    id: PrototypeId(0),
                    name: "inferred".into(),
                    shape: PrototypeShape::SimplePolygon {
                        vertices: vertices.to_vec(),
                    },
                }],
                instances: vec![crate::sim::tiling::TileInstance {
                    id: crate::sim::tiling::BasisId(0),
                    prototype: PrototypeId(0),
                    transform: crate::sim::tiling::RigidTransform::default(),
                }],
                mode: crate::sim::tiling::TilingMode::Topological,
            };
            if crate::sim::tiling::validate_coverage(&draft).is_ok() {
                return Ok((a, b));
            }
        }
    }
    Err(
        "could not infer an exact translation lattice; choose a preset or add/edit the periodic patch"
            .into(),
    )
}

/// Give an incomplete, multi-polygon unit cell a stable editable patch until
/// enough polygons exist to infer or explicitly edit its exact lattice.
pub fn provisional_translation_lattice(vertices: &[Vec2]) -> (Vec2, Vec2) {
    let min_x = vertices
        .iter()
        .map(|point| point.x)
        .fold(f64::INFINITY, f64::min);
    let max_x = vertices
        .iter()
        .map(|point| point.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = vertices
        .iter()
        .map(|point| point.y)
        .fold(f64::INFINITY, f64::min);
    let max_y = vertices
        .iter()
        .map(|point| point.y)
        .fold(f64::NEG_INFINITY, f64::max);
    (
        Vec2::new((max_x - min_x).max(1e-6), 0.0),
        Vec2::new(0.0, (max_y - min_y).max(1e-6)),
    )
}
