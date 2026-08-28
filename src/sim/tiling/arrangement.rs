use std::collections::{BTreeMap, BTreeSet};

use super::{
    Aabb, BasisId, GeometryBudget, LatticeCopyBounds, PeriodicTilingDraft, SegmentRelation,
    TilingDiagnostic, Vec2, polygon::instance_polygon, predicates::segment_relation,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VertexId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HalfEdgeId(pub usize);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ShapeEdgeRef {
    pub basis: BasisId,
    pub edge: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AtomicEdge {
    pub id: HalfEdgeId,
    pub source: ShapeEdgeRef,
    pub interval: [f64; 2],
    pub start: VertexId,
    pub end: VertexId,
    pub twin: HalfEdgeId,
    pub next: HalfEdgeId,
    pub offset: [i32; 2],
    pub geometry: [Vec2; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArrangementFace {
    pub basis: BasisId,
    pub boundary: Vec<HalfEdgeId>,
    pub signed_area: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NeighborPlacement {
    pub source_basis: BasisId,
    pub target_basis: BasisId,
    pub lattice_offset: [i32; 2],
    pub source_edge: HalfEdgeId,
    pub target_edge: HalfEdgeId,
    pub contact_length: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PeriodicArrangement {
    pub vertices: Vec<Vec2>,
    pub atomic_edges: Vec<AtomicEdge>,
    pub faces: Vec<ArrangementFace>,
}

#[derive(Clone, Copy)]
struct ShapeEdge {
    source: ShapeEdgeRef,
    start: Vec2,
    end: Vec2,
}

#[derive(Clone, Copy)]
struct Fragment {
    source: ShapeEdgeRef,
    interval: [f64; 2],
    geometry: [Vec2; 2],
}

impl PeriodicArrangement {
    pub fn build(
        draft: &PeriodicTilingDraft,
        budget: GeometryBudget,
    ) -> Result<Self, Vec<TilingDiagnostic>> {
        let mut diagnostics = Vec::new();
        let mut basis_ids = BTreeSet::new();
        let mut shape_edges = Vec::new();
        for (index, instance) in draft.instances.iter().enumerate() {
            if !basis_ids.insert(instance.id) {
                diagnostics.push(diagnostic_at(
                    "duplicate_basis",
                    format!("tile {} appears more than once", instance.id.0 + 1),
                    format!("basis/{}", instance.id.0),
                ));
                continue;
            }
            let polygon = match instance_polygon(draft, index) {
                Ok(polygon) => polygon,
                Err(issues) => {
                    diagnostics.extend(issues.into_iter().map(|issue| {
                        diagnostic_at(
                            issue.code,
                            issue.message,
                            format!("basis/{}", instance.id.0),
                        )
                    }));
                    continue;
                }
            };
            for edge in 0..polygon.vertices.len() {
                shape_edges.push(ShapeEdge {
                    source: ShapeEdgeRef {
                        basis: instance.id,
                        edge: edge as u16,
                    },
                    start: polygon.vertices[edge],
                    end: polygon.vertices[(edge + 1) % polygon.vertices.len()],
                });
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
        shape_edges.sort_by_key(|edge| edge.source);
        if shape_edges.is_empty() {
            return Err(vec![diagnostic(
                "empty_arrangement",
                "periodic arrangement has no shape edges",
            )]);
        }

        let mut split_parameters = vec![vec![0.0, 1.0]; shape_edges.len()];
        let mut pair_work = 0_usize;
        let mut relation_diagnostics = BTreeSet::new();
        for (left_index, left) in shape_edges.iter().enumerate() {
            let left_box = edge_aabb(*left);
            for (right_index, right) in shape_edges.iter().enumerate() {
                let bounds = match LatticeCopyBounds::for_aabb(
                    draft.translation_a,
                    draft.translation_b,
                    edge_aabb(*right),
                    left_box,
                    budget,
                ) {
                    Ok(bounds) => bounds,
                    Err(error) => return Err(vec![error]),
                };
                pair_work = match pair_work.checked_add(bounds.candidate_count()) {
                    Some(work) if work <= budget.max_segment_pairs => work,
                    _ => {
                        return Err(vec![diagnostic(
                            "budget_segment_pairs",
                            "periodic segment comparisons exceed the geometry budget",
                        )]);
                    }
                };
                for offset in bounds.iter() {
                    if left_index == right_index && offset == [0, 0] {
                        continue;
                    }
                    let shift = lattice_shift(draft, offset);
                    let right_start = snap_to_edge_endpoints(right.start + shift, *left, draft);
                    let right_end = snap_to_edge_endpoints(right.end + shift, *left, draft);
                    match segment_relation(left.start, left.end, right_start, right_end) {
                        SegmentRelation::ProperCrossing => {
                            let key = ordered_relation_key(left.source, right.source, offset);
                            if relation_diagnostics.insert(("proper_crossing", key)) {
                                diagnostics.push(diagnostic_at(
                                    "proper_crossing",
                                    format!(
                                        "{} crosses through {} in {}",
                                        edge_name(left.source),
                                        edge_name(right.source),
                                        offset_name(offset)
                                    ),
                                    edge_path(left.source),
                                ));
                            }
                        }
                        SegmentRelation::TEndpoint => {
                            let key = ordered_relation_key(left.source, right.source, offset);
                            if relation_diagnostics.insert(("t_junction", key)) {
                                diagnostics.push(diagnostic_at(
                                    "t_junction",
                                    format!(
                                        "a corner of {} lands part-way along {} in {}, instead of meeting it end to end",
                                        edge_name(left.source),
                                        edge_name(right.source),
                                        offset_name(offset)
                                    ),
                                    edge_path(left.source),
                                ));
                            }
                        }
                        SegmentRelation::CollinearOverlap => {
                            if (left.end - left.start).dot(right_end - right_start) > 0.0 {
                                let key = ordered_relation_key(left.source, right.source, offset);
                                if relation_diagnostics
                                    .insert(("incompatible_collinear_overlap", key))
                                {
                                    diagnostics.push(diagnostic_at(
                                        "incompatible_collinear_overlap",
                                        "collinear boundary fragments run in the same direction",
                                        edge_path(left.source),
                                    ));
                                }
                            }
                        }
                        SegmentRelation::Disjoint | SegmentRelation::Endpoint => {}
                    }
                }
            }
        }
        if diagnostics
            .iter()
            .any(|diagnostic| matches!(diagnostic.code, "proper_crossing" | "t_junction"))
        {
            return Err(diagnostics);
        }

        let mut fragments = Vec::new();
        for (edge, parameters) in shape_edges.iter().zip(&mut split_parameters) {
            parameters.sort_by(f64::total_cmp);
            parameters.dedup_by(|left, right| (*left - *right).abs() <= 1e-12);
            for interval in parameters.windows(2) {
                let start = interval[0].clamp(0.0, 1.0);
                let end = interval[1].clamp(0.0, 1.0);
                let geometry = [interpolate(*edge, start), interpolate(*edge, end)];
                if end <= start || points_close(geometry[0], geometry[1], draft) {
                    diagnostics.push(diagnostic_at(
                        "zero_length_fragment",
                        "edge splitting produced a zero-length atomic fragment",
                        edge_path(edge.source),
                    ));
                    continue;
                }
                if fragments.len() == budget.max_atomic_edges {
                    return Err(vec![diagnostic(
                        "budget_atomic_edges",
                        "atomic edge count exceeds the geometry budget",
                    )]);
                }
                fragments.push(Fragment {
                    source: edge.source,
                    interval: [start, end],
                    geometry,
                });
            }
        }
        if diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "zero_length_fragment")
        {
            return Err(diagnostics);
        }

        let comparison_count = fragments
            .len()
            .checked_mul(fragments.len())
            .ok_or_else(|| {
                vec![diagnostic(
                    "budget_segment_pairs",
                    "atomic twin comparison count overflow",
                )]
            })?;
        if comparison_count > budget.max_segment_pairs {
            return Err(vec![diagnostic(
                "budget_segment_pairs",
                "atomic twin comparisons exceed the geometry budget",
            )]);
        }
        let mut twins = Vec::with_capacity(fragments.len());
        for (index, fragment) in fragments.iter().enumerate() {
            let mut candidates = Vec::new();
            for (other_index, other) in fragments.iter().enumerate() {
                if let Some(offset) = reversed_lattice_match(*fragment, *other, draft)
                    && (index != other_index || offset != [0, 0])
                {
                    candidates.push((other_index, offset));
                }
            }
            candidates.sort_unstable();
            candidates.dedup();
            match candidates.as_slice() {
                [] => diagnostics.push(diagnostic_at(
                    "unmatched_atomic_edge",
                    format!(
                        "{} has no matching edge to glue to in the next copy",
                        edge_name(fragment.source)
                    ),
                    edge_path(fragment.source),
                )),
                [(other, offset)] => twins.push((HalfEdgeId(*other), *offset)),
                _ => diagnostics.push(diagnostic_at(
                    "competing_twins",
                    format!(
                        "{} could glue to {} different edges, so the pairing is ambiguous",
                        edge_name(fragment.source),
                        candidates.len()
                    ),
                    edge_path(fragment.source),
                )),
            }
        }
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }

        let mut vertices = Vec::new();
        let mut vertex_buckets: BTreeMap<(i64, i64), VertexId> = BTreeMap::new();
        let mut starts = Vec::with_capacity(fragments.len());
        let mut ends = Vec::with_capacity(fragments.len());
        for fragment in &fragments {
            starts.push(intern_vertex(
                fragment.geometry[0],
                draft,
                &mut vertices,
                &mut vertex_buckets,
            ));
            ends.push(intern_vertex(
                fragment.geometry[1],
                draft,
                &mut vertices,
                &mut vertex_buckets,
            ));
        }

        let mut next = vec![HalfEdgeId(0); fragments.len()];
        let mut by_basis: BTreeMap<BasisId, Vec<usize>> = BTreeMap::new();
        for (index, fragment) in fragments.iter().enumerate() {
            by_basis
                .entry(fragment.source.basis)
                .or_default()
                .push(index);
        }
        if by_basis.len() > budget.max_faces {
            return Err(vec![diagnostic(
                "budget_faces",
                "arrangement face count exceeds the geometry budget",
            )]);
        }
        let mut faces = Vec::with_capacity(by_basis.len());
        for (basis, boundary) in &mut by_basis {
            boundary.sort_by(|left, right| {
                fragments[*left]
                    .source
                    .edge
                    .cmp(&fragments[*right].source.edge)
                    .then_with(|| {
                        fragments[*left].interval[0].total_cmp(&fragments[*right].interval[0])
                    })
            });
            for position in 0..boundary.len() {
                next[boundary[position]] = HalfEdgeId(boundary[(position + 1) % boundary.len()]);
            }
            let signed_area = boundary
                .iter()
                .map(|index| fragments[*index].geometry[0].cross(fragments[*index].geometry[1]))
                .sum::<f64>()
                * 0.5;
            faces.push(ArrangementFace {
                basis: *basis,
                boundary: boundary.iter().map(|index| HalfEdgeId(*index)).collect(),
                signed_area,
            });
        }

        let atomic_edges = fragments
            .iter()
            .enumerate()
            .map(|(index, fragment)| AtomicEdge {
                id: HalfEdgeId(index),
                source: fragment.source,
                interval: fragment.interval,
                start: starts[index],
                end: ends[index],
                twin: twins[index].0,
                next: next[index],
                offset: twins[index].1,
                geometry: fragment.geometry,
            })
            .collect::<Vec<_>>();
        for edge in &atomic_edges {
            let twin = &atomic_edges[edge.twin.0];
            if twin.twin != edge.id || twin.offset != [-edge.offset[0], -edge.offset[1]] {
                return Err(vec![diagnostic_at(
                    "asymmetric_twin",
                    "paired atomic edges do not agree on their inverse periodic offset",
                    edge_path(edge.source),
                )]);
            }
        }
        Ok(Self {
            vertices,
            atomic_edges,
            faces,
        })
    }

    pub fn neighbor_ring(&self, basis: BasisId) -> Vec<NeighborPlacement> {
        let mut neighbors = self
            .atomic_edges
            .iter()
            .filter(|edge| edge.source.basis == basis)
            .map(|edge| {
                let twin = &self.atomic_edges[edge.twin.0];
                NeighborPlacement {
                    source_basis: basis,
                    target_basis: twin.source.basis,
                    lattice_offset: edge.offset,
                    source_edge: edge.id,
                    target_edge: twin.id,
                    contact_length: (edge.geometry[1] - edge.geometry[0]).length(),
                }
            })
            .collect::<Vec<_>>();
        neighbors.sort_by_key(|neighbor| {
            (
                neighbor.target_basis,
                neighbor.lattice_offset,
                neighbor.source_edge,
            )
        });
        neighbors
    }
}

fn edge_aabb(edge: ShapeEdge) -> Aabb {
    Aabb {
        min: Vec2::new(edge.start.x.min(edge.end.x), edge.start.y.min(edge.end.y)),
        max: Vec2::new(edge.start.x.max(edge.end.x), edge.start.y.max(edge.end.y)),
    }
}

fn snap_to_edge_endpoints(point: Vec2, edge: ShapeEdge, draft: &PeriodicTilingDraft) -> Vec2 {
    if points_close(point, edge.start, draft) {
        edge.start
    } else if points_close(point, edge.end, draft) {
        edge.end
    } else {
        point
    }
}

fn interpolate(edge: ShapeEdge, parameter: f64) -> Vec2 {
    edge.start + (edge.end - edge.start) * parameter
}

fn lattice_shift(draft: &PeriodicTilingDraft, offset: [i32; 2]) -> Vec2 {
    draft.translation_a * f64::from(offset[0]) + draft.translation_b * f64::from(offset[1])
}

fn reversed_lattice_match(
    left: Fragment,
    right: Fragment,
    draft: &PeriodicTilingDraft,
) -> Option<[i32; 2]> {
    let first_delta = left.geometry[0] - right.geometry[1];
    let second_delta = left.geometry[1] - right.geometry[0];
    if !points_close(first_delta, second_delta, draft) {
        return None;
    }
    let determinant = draft.translation_a.cross(draft.translation_b);
    let coordinates = [
        first_delta.cross(draft.translation_b) / determinant,
        draft.translation_a.cross(first_delta) / determinant,
    ];
    let rounded = [coordinates[0].round(), coordinates[1].round()];
    if rounded
        .iter()
        .any(|value| *value < f64::from(i32::MIN) || *value > f64::from(i32::MAX))
    {
        return None;
    }
    let offset = [rounded[0] as i32, rounded[1] as i32];
    let shift = lattice_shift(draft, offset);
    (points_close(left.geometry[0], right.geometry[1] + shift, draft)
        && points_close(left.geometry[1], right.geometry[0] + shift, draft))
    .then_some(offset)
}

fn points_close(left: Vec2, right: Vec2, draft: &PeriodicTilingDraft) -> bool {
    let coordinate_scale = [left.x, left.y, right.x, right.y]
        .into_iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()));
    let local_scale = draft
        .translation_a
        .length()
        .max(draft.translation_b.length())
        .max(1e-300);
    let tolerance = local_scale * 1e-10 + coordinate_scale * f64::EPSILON * 4.0;
    (left - right).length() <= tolerance
}

fn intern_vertex(
    point: Vec2,
    draft: &PeriodicTilingDraft,
    vertices: &mut Vec<Vec2>,
    buckets: &mut BTreeMap<(i64, i64), VertexId>,
) -> VertexId {
    let determinant = draft.translation_a.cross(draft.translation_b);
    let mut coordinate = [
        point.cross(draft.translation_b) / determinant,
        draft.translation_a.cross(point) / determinant,
    ];
    for value in &mut coordinate {
        *value = value.rem_euclid(1.0);
        if *value <= 1e-10 || (1.0 - *value) <= 1e-10 {
            *value = 0.0;
        }
    }
    let key = (
        (coordinate[0] * 1e10).round() as i64,
        (coordinate[1] * 1e10).round() as i64,
    );
    if let Some(id) = buckets.get(&key) {
        return *id;
    }
    let id = VertexId(vertices.len());
    vertices.push(draft.translation_a * coordinate[0] + draft.translation_b * coordinate[1]);
    buckets.insert(key, id);
    id
}

fn ordered_relation_key(
    left: ShapeEdgeRef,
    right: ShapeEdgeRef,
    offset: [i32; 2],
) -> (ShapeEdgeRef, ShapeEdgeRef, [i32; 2]) {
    if left <= right {
        (left, right, offset)
    } else {
        (right, left, [-offset[0], -offset[1]])
    }
}

fn edge_path(edge: ShapeEdgeRef) -> String {
    format!("basis/{}/edge/{}", edge.basis.0, edge.edge)
}

/// Name an edge the way the user sees it on the canvas.
///
/// These strings reach the tiling verdict in the window, so they say "edge 2 of
/// tile 1" rather than printing the internal reference. A reader who has to
/// decode `ShapeEdgeRef { basis: TileId(1), edge: 2 }` is reading our notes,
/// not a description of their drawing.
fn edge_name(edge: ShapeEdgeRef) -> String {
    format!("edge {} of tile {}", edge.edge + 1, edge.basis.0 + 1)
}

/// Describe which repeat of the tiling the other edge belongs to.
fn offset_name(offset: [i32; 2]) -> String {
    match offset {
        [0, 0] => "the same copy".to_string(),
        [a, b] => format!("the copy {a} across and {b} up"),
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> TilingDiagnostic {
    TilingDiagnostic {
        code,
        message: message.into(),
        path: None,
    }
}

fn diagnostic_at(
    code: &'static str,
    message: impl Into<String>,
    path: impl Into<String>,
) -> TilingDiagnostic {
    TilingDiagnostic {
        code,
        message: message.into(),
        path: Some(path.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{
        GeometryBudget, PeriodicTilingDraft, PrototypeId, PrototypeShape, RigidTransform, TileId,
        TileInstance, TilePrototype, TilingMode, TilingPreset, Vec2, build_preset,
    };

    fn prototype(id: u32, vertices: &[[f64; 2]]) -> TilePrototype {
        TilePrototype {
            id: PrototypeId(id),
            name: format!("shape-{id}"),
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

    fn t_junction_fixture() -> PeriodicTilingDraft {
        PeriodicTilingDraft {
            translation_a: Vec2::new(2.0, 0.0),
            translation_b: Vec2::new(0.0, 2.0),
            prototypes: vec![
                prototype(0, &[[0.0, 0.0], [1.0, 0.0], [1.0, 2.0], [0.0, 2.0]]),
                prototype(1, &[[1.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0]]),
                prototype(2, &[[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]]),
            ],
            instances: vec![instance(0), instance(1), instance(2)],
            mode: TilingMode::Topological,
        }
    }

    fn hexagon_fixture() -> PeriodicTilingDraft {
        PeriodicTilingDraft {
            translation_a: Vec2::new(1.5, 3.0_f64.sqrt() / 2.0),
            translation_b: Vec2::new(0.0, 3.0_f64.sqrt()),
            prototypes: vec![TilePrototype {
                id: PrototypeId(0),
                name: "hexagon".into(),
                shape: PrototypeShape::RegularPolygon {
                    sides: 6,
                    side_length: 1.0,
                },
            }],
            instances: vec![instance(0)],
            mode: TilingMode::Topological,
        }
    }

    #[test]
    fn square_hexagon_and_octagon_square_have_unique_twins() {
        for (name, draft) in [
            ("square", build_preset(TilingPreset::Square, 1.0)),
            ("hexagon", hexagon_fixture()),
            (
                "octagon-square",
                build_preset(TilingPreset::OctagonSquare, 1.0),
            ),
        ] {
            let arrangement = PeriodicArrangement::build(&draft, GeometryBudget::authoritative())
                .unwrap_or_else(|errors| panic!("{name}: {errors:?}"));
            assert!(
                arrangement
                    .atomic_edges
                    .iter()
                    .all(|edge| { arrangement.atomic_edges[edge.twin.0].twin == edge.id })
            );
            assert_eq!(arrangement.faces.len(), draft.instances.len());
        }
    }

    #[test]
    fn t_junction_is_rejected_in_strict_edge_to_edge_mode() {
        let errors =
            PeriodicArrangement::build(&t_junction_fixture(), GeometryBudget::authoritative())
                .unwrap_err();
        assert!(errors.iter().any(|error| error.code == "t_junction"));
    }

    #[test]
    fn proper_crossing_is_rejected_with_stable_code() {
        let mut draft = build_preset(TilingPreset::Square, 2.0);
        draft.prototypes.push(prototype(
            1,
            &[[0.5, -0.5], [2.5, 0.5], [1.5, 2.5], [-0.5, 1.5]],
        ));
        draft.instances.push(instance(1));
        let errors =
            PeriodicArrangement::build(&draft, GeometryBudget::authoritative()).unwrap_err();
        assert!(errors.iter().any(|error| error.code == "proper_crossing"));
    }

    #[test]
    fn unmatched_and_competing_twins_are_rejected() {
        let triangle = PeriodicTilingDraft {
            translation_a: Vec2::new(2.0, 0.0),
            translation_b: Vec2::new(0.0, 2.0),
            prototypes: vec![prototype(0, &[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]])],
            instances: vec![instance(0)],
            mode: TilingMode::Topological,
        };
        assert!(
            PeriodicArrangement::build(&triangle, GeometryBudget::authoritative())
                .unwrap_err()
                .iter()
                .any(|error| error.code == "unmatched_atomic_edge")
        );

        let mut duplicate = build_preset(TilingPreset::Square, 1.0);
        duplicate.instances.push(TileInstance {
            id: TileId(1),
            prototype: PrototypeId(0),
            transform: RigidTransform::default(),
        });
        let errors =
            PeriodicArrangement::build(&duplicate, GeometryBudget::authoritative()).unwrap_err();
        assert!(errors.iter().any(|error| error.code == "competing_twins"));
        assert!(
            errors
                .iter()
                .any(|error| error.code == "incompatible_collinear_overlap")
        );
    }

    #[test]
    fn segment_pair_budget_fails_before_quadratic_work() {
        let budget = GeometryBudget {
            max_segment_pairs: 1,
            ..GeometryBudget::interactive()
        };
        let errors = PeriodicArrangement::build(&t_junction_fixture(), budget).unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.code == "budget_segment_pairs")
        );

        let face_budget = GeometryBudget {
            max_faces: 1,
            ..GeometryBudget::authoritative()
        };
        let errors = PeriodicArrangement::build(
            &build_preset(TilingPreset::OctagonSquare, 1.0),
            face_budget,
        )
        .unwrap_err();
        assert!(errors.iter().any(|error| error.code == "budget_faces"));
    }
}
