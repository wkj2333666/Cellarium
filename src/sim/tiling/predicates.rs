use std::cmp::Ordering;

use robust::{Coord, orient2d};

use super::Vec2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentRelation {
    Disjoint,
    Endpoint,
    ProperCrossing,
    TEndpoint,
    CollinearOverlap,
}

pub fn segment_relation(a0: Vec2, a1: Vec2, b0: Vec2, b1: Vec2) -> SegmentRelation {
    if ![a0, a1, b0, b1]
        .iter()
        .all(|point| point.x.is_finite() && point.y.is_finite())
    {
        return SegmentRelation::Disjoint;
    }
    let shared_endpoint =
        point_equal(a0, b0) || point_equal(a0, b1) || point_equal(a1, b0) || point_equal(a1, b1);
    let orientation = [
        sign(a0, a1, b0),
        sign(a0, a1, b1),
        sign(b0, b1, a0),
        sign(b0, b1, a1),
    ];
    if orientation.iter().all(|value| *value == Ordering::Equal) {
        let use_x = (a1.x - a0.x).abs().max((b1.x - b0.x).abs())
            >= (a1.y - a0.y).abs().max((b1.y - b0.y).abs());
        let coordinate = |point: Vec2| if use_x { point.x } else { point.y };
        let (a_min, a_max) = ordered(coordinate(a0), coordinate(a1));
        let (b_min, b_max) = ordered(coordinate(b0), coordinate(b1));
        let low = a_min.max(b_min);
        let high = a_max.min(b_max);
        let tolerance = scalar_tolerance(&[a_min, a_max, b_min, b_max]);
        if low - high > tolerance {
            SegmentRelation::Disjoint
        } else if (high - low).abs() <= tolerance {
            SegmentRelation::Endpoint
        } else {
            SegmentRelation::CollinearOverlap
        }
    } else if shared_endpoint {
        SegmentRelation::Endpoint
    } else if opposite(orientation[0], orientation[1]) && opposite(orientation[2], orientation[3]) {
        SegmentRelation::ProperCrossing
    } else {
        let a0_on_b = orientation[2] == Ordering::Equal && in_bounds(a0, b0, b1);
        let a1_on_b = orientation[3] == Ordering::Equal && in_bounds(a1, b0, b1);
        let b0_on_a = orientation[0] == Ordering::Equal && in_bounds(b0, a0, a1);
        let b1_on_a = orientation[1] == Ordering::Equal && in_bounds(b1, a0, a1);
        if a0_on_b || a1_on_b || b0_on_a || b1_on_a {
            SegmentRelation::TEndpoint
        } else {
            SegmentRelation::Disjoint
        }
    }
}

pub(crate) fn point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> bool {
    sign(start, end, point) == Ordering::Equal && in_bounds(point, start, end)
}

fn sign(a: Vec2, b: Vec2, c: Vec2) -> Ordering {
    orient2d(
        Coord { x: a.x, y: a.y },
        Coord { x: b.x, y: b.y },
        Coord { x: c.x, y: c.y },
    )
    .partial_cmp(&0.0)
    .unwrap_or(Ordering::Equal)
}

fn opposite(left: Ordering, right: Ordering) -> bool {
    matches!(
        (left, right),
        (Ordering::Less, Ordering::Greater) | (Ordering::Greater, Ordering::Less)
    )
}

fn in_bounds(point: Vec2, start: Vec2, end: Vec2) -> bool {
    let tolerance = scalar_tolerance(&[point.x, point.y, start.x, start.y, end.x, end.y]);
    point.x >= start.x.min(end.x) - tolerance
        && point.x <= start.x.max(end.x) + tolerance
        && point.y >= start.y.min(end.y) - tolerance
        && point.y <= start.y.max(end.y) + tolerance
}

fn point_equal(left: Vec2, right: Vec2) -> bool {
    let tolerance = scalar_tolerance(&[left.x, left.y, right.x, right.y]);
    (left - right).length() <= tolerance
}

fn scalar_tolerance(values: &[f64]) -> f64 {
    values
        .iter()
        .fold(1.0_f64, |scale, value| scale.max(value.abs()))
        * f64::EPSILON
        * 3.5
}

fn ordered(left: f64, right: f64) -> (f64, f64) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    #[test]
    fn classifies_endpoint_crossing_t_and_overlap_relations() {
        assert_eq!(
            segment_relation(
                point(0.0, 0.0),
                point(2.0, 0.0),
                point(2.0, 0.0),
                point(2.0, 1.0)
            ),
            SegmentRelation::Endpoint
        );
        assert_eq!(
            segment_relation(
                point(0.0, 0.0),
                point(2.0, 2.0),
                point(0.0, 2.0),
                point(2.0, 0.0)
            ),
            SegmentRelation::ProperCrossing
        );
        assert_eq!(
            segment_relation(
                point(0.0, 0.0),
                point(2.0, 0.0),
                point(1.0, 0.0),
                point(1.0, 1.0)
            ),
            SegmentRelation::TEndpoint
        );
        assert_eq!(
            segment_relation(
                point(0.0, 0.0),
                point(3.0, 0.0),
                point(2.0, 0.0),
                point(1.0, 0.0)
            ),
            SegmentRelation::CollinearOverlap
        );
    }

    #[test]
    fn relation_is_invariant_under_reversal_translation_rotation_and_scale() {
        let original = [
            point(0.0, 0.0),
            point(3.0, 0.0),
            point(1.0, -2.0),
            point(1.0, 2.0),
        ];
        let expected = SegmentRelation::ProperCrossing;
        for scale in [1e-9, 1.0, 1e9] {
            let angle: f64 = 0.731;
            let transform = |p: Vec2| {
                let (sin, cos) = angle.sin_cos();
                Vec2::new(
                    scale * (p.x * cos - p.y * sin) + 9.0 * scale,
                    scale * (p.x * sin + p.y * cos) - 4.0 * scale,
                )
            };
            let p = original.map(transform);
            assert_eq!(segment_relation(p[0], p[1], p[2], p[3]), expected);
            assert_eq!(segment_relation(p[1], p[0], p[3], p[2]), expected);
        }
    }

    #[test]
    fn adaptive_orientation_handles_almost_collinear_large_coordinates() {
        let base = 1e15;
        assert_eq!(
            segment_relation(
                point(base, base),
                point(base + 8.0, base + 8.0),
                point(base, base + 8.0),
                point(base + 8.0, base),
            ),
            SegmentRelation::ProperCrossing
        );
    }

    #[test]
    fn large_absolute_coordinates_do_not_merge_distinct_segments() {
        let base = 1e16;
        assert_eq!(
            segment_relation(
                point(base, 0.0),
                point(base + 8.0, 0.0),
                point(base + 16.0, 0.0),
                point(base + 24.0, 0.0),
            ),
            SegmentRelation::Disjoint
        );
    }
}
