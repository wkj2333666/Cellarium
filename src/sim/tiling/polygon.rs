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
            if segments_intersect_or_touch(a0, a1, b0, b1) {
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

/// Validate one click before it becomes part of an open polygon path.
/// Closing is a separate action: clicking the first handle never appends a
/// duplicate endpoint.
pub fn validate_open_path_append(vertices: &[Vec2], point: Vec2) -> Result<(), String> {
    const EPS: f64 = 1e-12;
    if !point.x.is_finite() || !point.y.is_finite() {
        return Err("vertex coordinates must be finite".into());
    }
    if vertices.len() >= MAX_POLYGON_VERTICES {
        return Err(format!(
            "polygon may contain at most {MAX_POLYGON_VERTICES} vertices"
        ));
    }
    if vertices
        .iter()
        .any(|existing| (point - *existing).length() <= EPS)
    {
        return Err("vertex overlaps an existing vertex; click the first handle to close".into());
    }
    let Some(last) = vertices.last().copied() else {
        return Ok(());
    };
    for edge in vertices.windows(2).take(vertices.len().saturating_sub(2)) {
        if segments_intersect_or_touch(edge[0], edge[1], last, point) {
            return Err("new edge crosses or touches the open polygon path".into());
        }
    }
    Ok(())
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

fn segments_intersect_or_touch(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    const EPS: f64 = 1e-12;
    let orientation = |p: Vec2, q: Vec2, r: Vec2| (q - p).cross(r - p);
    let on_segment = |p: Vec2, q: Vec2, r: Vec2| {
        orientation(p, q, r).abs() <= EPS
            && r.x >= p.x.min(q.x) - EPS
            && r.x <= p.x.max(q.x) + EPS
            && r.y >= p.y.min(q.y) - EPS
            && r.y <= p.y.max(q.y) + EPS
    };
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);
    ((o1 > EPS && o2 < -EPS) || (o1 < -EPS && o2 > EPS))
        && ((o3 > EPS && o4 < -EPS) || (o3 < -EPS && o4 > EPS))
        || on_segment(a, b, c)
        || on_segment(a, b, d)
        || on_segment(c, d, a)
        || on_segment(c, d, b)
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
