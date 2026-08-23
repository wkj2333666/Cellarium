use super::{GeometryIssue, PeriodicTilingDraft, PrototypeShape, RigidTransform, Vec2};

pub const MAX_POLYGON_VERTICES: usize = 64;
/// Minimal owned polygon adapter.  The public model deliberately does not
/// expose a third-party geometry type; this boundary can be swapped for geo
/// when the dependency is available in an offline build cache.
#[derive(Clone, Debug, PartialEq)]
pub struct Polygon {
    pub vertices: Vec<Vec2>,
}

impl Polygon {
    pub fn unsigned_area(&self) -> f64 {
        signed_area(&self.vertices).abs()
    }
}

pub fn prototype_vertices(shape: &PrototypeShape) -> Result<Vec<Vec2>, Vec<GeometryIssue>> {
    match shape {
        PrototypeShape::RegularPolygon { sides, side_length } => {
            if !(3..=64).contains(sides) {
                return Err(vec![issue(
                    "invalid_sides",
                    "polygon sides must be between 3 and 64",
                    None,
                )]);
            }
            if !side_length.is_finite() || *side_length <= 0.0 {
                return Err(vec![issue(
                    "invalid_side_length",
                    "side length must be finite and positive",
                    None,
                )]);
            }
            let radius = *side_length / (std::f64::consts::PI / f64::from(*sides)).sin() / 2.0;
            let phase = std::f64::consts::FRAC_PI_2 - std::f64::consts::PI / f64::from(*sides);
            Ok((0..*sides)
                .map(|i| {
                    let angle =
                        phase + 2.0 * std::f64::consts::PI * f64::from(i) / f64::from(*sides);
                    Vec2::new(radius * angle.cos(), radius * angle.sin())
                })
                .collect())
        }
        PrototypeShape::SimplePolygon { vertices } => {
            if vertices.len() > MAX_POLYGON_VERTICES {
                return Err(vec![issue(
                    "too_many_vertices",
                    "polygon may contain at most 64 vertices",
                    None,
                )]);
            }
            Ok(vertices.clone())
        }
    }
}

pub fn instance_polygon(
    draft: &PeriodicTilingDraft,
    instance_index: usize,
) -> Result<Polygon, Vec<GeometryIssue>> {
    let instance = draft
        .instances
        .get(instance_index)
        .ok_or_else(|| vec![issue("unknown_tile", "tile instance does not exist", None)])?;
    let prototype = draft
        .prototypes
        .iter()
        .find(|p| p.id == instance.prototype)
        .ok_or_else(|| {
            vec![issue(
                "unknown_prototype",
                "tile references an unknown prototype",
                None,
            )]
        })?;
    let vertices = prototype_vertices(&prototype.shape)?;
    let transformed = transform_vertices(&vertices, instance.transform);
    let issues = validate_polygon(&transformed);
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(to_geo_polygon(&transformed))
}

pub fn transform_vertices(vertices: &[Vec2], transform: RigidTransform) -> Vec<Vec2> {
    let (s, c) = transform.rotation.sin_cos();
    vertices
        .iter()
        .map(|v| Vec2::new(c * v.x - s * v.y, s * v.x + c * v.y) + transform.translation)
        .collect()
}

pub fn validate_polygon(vertices: &[Vec2]) -> Vec<GeometryIssue> {
    let mut issues = Vec::new();
    if vertices.len() > MAX_POLYGON_VERTICES {
        issues.push(issue(
            "too_many_vertices",
            "polygon may contain at most 64 vertices",
            None,
        ));
        return issues;
    }
    if vertices.len() < 3 {
        issues.push(issue(
            "too_few_vertices",
            "polygon needs at least three vertices",
            None,
        ));
        return issues;
    }
    if vertices
        .iter()
        .any(|v| !v.x.is_finite() || !v.y.is_finite())
    {
        issues.push(issue(
            "non_finite_vertex",
            "polygon vertices must be finite",
            None,
        ));
        return issues;
    }
    let area = signed_area(vertices);
    if area <= 1e-14 {
        issues.push(issue(
            "non_ccw_or_degenerate",
            "polygon must be counter-clockwise with non-zero area",
            None,
        ));
    }
    for i in 0..vertices.len() {
        let a0 = vertices[i];
        let a1 = vertices[(i + 1) % vertices.len()];
        if (a1 - a0).length() <= 1e-12 {
            issues.push(issue(
                "zero_edge",
                "polygon contains a zero-length edge",
                Some(i),
            ));
        }
        for j in (i + 1)..vertices.len() {
            if j == i + 1 || (i == 0 && j + 1 == vertices.len()) {
                continue;
            }
            let b0 = vertices[j];
            let b1 = vertices[(j + 1) % vertices.len()];
            if segments_intersect(a0, a1, b0, b1) {
                issues.push(issue(
                    "self_intersection",
                    "polygon edges intersect",
                    Some(i),
                ));
            }
        }
    }
    // Keep the adapter boundary exercised: geo's area must agree with the
    // owned shoelace area before a polygon is accepted.
    if issues.is_empty() {
        let geo_area = to_geo_polygon(vertices).unsigned_area();
        if !geo_area.is_finite() || (geo_area - area).abs() > area.abs() * 1e-10 + 1e-12 {
            issues.push(issue(
                "invalid_geometry",
                "geometry adapter rejected polygon",
                None,
            ));
        }
    }
    issues
}

pub fn signed_area(vertices: &[Vec2]) -> f64 {
    vertices
        .iter()
        .enumerate()
        .map(|(i, a)| a.cross(vertices[(i + 1) % vertices.len()]))
        .sum::<f64>()
        * 0.5
}

pub fn cyclic_edges(vertices: &[Vec2]) -> impl Iterator<Item = Vec2> + '_ {
    vertices
        .iter()
        .enumerate()
        .map(|(i, a)| vertices[(i + 1) % vertices.len()] - *a)
}

pub fn to_geo_polygon(vertices: &[Vec2]) -> Polygon {
    Polygon {
        vertices: vertices.to_vec(),
    }
}

fn issue(code: &'static str, message: &str, vertex: Option<usize>) -> GeometryIssue {
    GeometryIssue {
        code,
        message: message.into(),
        vertex,
    }
}

fn segments_intersect(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let ab = b - a;
    let ac = c - a;
    let ad = d - a;
    let cd = d - c;
    let ca = a - c;
    let cb = b - c;
    let o1 = ab.cross(ac);
    let o2 = ab.cross(ad);
    let o3 = cd.cross(ca);
    let o4 = cd.cross(cb);
    let eps = 1e-12;
    ((o1 > eps && o2 < -eps) || (o1 < -eps && o2 > eps))
        && ((o3 > eps && o4 < -eps) || (o3 < -eps && o4 > eps))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn v(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn regular_octagon_has_unit_edges_and_positive_area() {
        let shape = PrototypeShape::RegularPolygon {
            sides: 8,
            side_length: 1.0,
        };
        let vertices = prototype_vertices(&shape).unwrap();
        assert_eq!(vertices.len(), 8);
        for edge in cyclic_edges(&vertices) {
            assert!((edge.length() - 1.0).abs() < 1e-12);
        }
        assert!(signed_area(&vertices) > 0.0);
    }

    #[test]
    fn custom_bow_tie_is_rejected_as_self_intersecting() {
        let vertices = vec![v(0.0, 0.0), v(1.0, 1.0), v(0.0, 1.0), v(1.0, 0.0)];
        assert!(
            validate_polygon(&vertices)
                .iter()
                .any(|i| i.code == "self_intersection")
        );
    }

    #[test]
    fn oversized_custom_polygon_is_rejected_before_quadratic_validation() {
        let vertices = (0..=MAX_POLYGON_VERTICES)
            .map(|index| {
                let angle =
                    2.0 * std::f64::consts::PI * index as f64 / (MAX_POLYGON_VERTICES + 1) as f64;
                Vec2::new(angle.cos(), angle.sin())
            })
            .collect::<Vec<_>>();
        let issues = validate_polygon(&vertices);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].code, "too_many_vertices");
        let shape = PrototypeShape::SimplePolygon { vertices };
        assert_eq!(
            prototype_vertices(&shape).unwrap_err()[0].code,
            "too_many_vertices"
        );
    }
}
