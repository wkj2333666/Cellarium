use super::{TilingDiagnostic, Vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec2,
    pub max: Vec2,
}

impl Aabb {
    pub fn new(min: Vec2, max: Vec2) -> Result<Self, TilingDiagnostic> {
        if !min.x.is_finite()
            || !min.y.is_finite()
            || !max.x.is_finite()
            || !max.y.is_finite()
            || min.x > max.x
            || min.y > max.y
        {
            return Err(diagnostic(
                "invalid_aabb",
                "AABB bounds must be finite and ordered",
            ));
        }
        Ok(Self { min, max })
    }

    pub fn translated(self, offset: Vec2) -> Self {
        Self {
            min: self.min + offset,
            max: self.max + offset,
        }
    }

    pub fn intersects(self, other: Self) -> bool {
        self.min.x <= other.max.x
            && self.max.x >= other.min.x
            && self.min.y <= other.max.y
            && self.max.y >= other.min.y
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeometryBudget {
    pub max_candidate_copies: usize,
    pub max_segment_pairs: usize,
    pub max_atomic_edges: usize,
    pub max_faces: usize,
}

impl GeometryBudget {
    pub const fn interactive() -> Self {
        Self {
            max_candidate_copies: 16_384,
            max_segment_pairs: 65_536,
            max_atomic_edges: 16_384,
            max_faces: 8_192,
        }
    }

    pub const fn authoritative() -> Self {
        Self {
            max_candidate_copies: 1_000_000,
            max_segment_pairs: 4_000_000,
            max_atomic_edges: 1_000_000,
            max_faces: 500_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LatticeCopyBounds {
    pub lower: [i32; 2],
    pub upper: [i32; 2],
    candidate_count: usize,
}

impl LatticeCopyBounds {
    pub fn for_aabb(
        a: Vec2,
        b: Vec2,
        source: Aabb,
        target: Aabb,
        budget: GeometryBudget,
    ) -> Result<Self, TilingDiagnostic> {
        if !a.x.is_finite() || !a.y.is_finite() || !b.x.is_finite() || !b.y.is_finite() {
            return Err(diagnostic(
                "invalid_period",
                "lattice vectors must contain finite coordinates",
            ));
        }
        let determinant = a.cross(b);
        if !determinant.is_finite() || determinant == 0.0 {
            return Err(diagnostic(
                "invalid_period",
                "lattice vectors must be linearly independent",
            ));
        }

        // A translated source AABB intersects the target exactly when the
        // translation lies in target - source. Transform that Minkowski AABB
        // through the inverse lattice and bound its four corners. The result
        // is a conservative integer superset for skew lattices.
        let difference = Aabb {
            min: Vec2::new(target.min.x - source.max.x, target.min.y - source.max.y),
            max: Vec2::new(target.max.x - source.min.x, target.max.y - source.min.y),
        };
        let corners = [
            difference.min,
            Vec2::new(difference.max.x, difference.min.y),
            difference.max,
            Vec2::new(difference.min.x, difference.max.y),
        ];
        let mut minimum = [f64::INFINITY; 2];
        let mut maximum = [f64::NEG_INFINITY; 2];
        for point in corners {
            let coordinate = [point.cross(b) / determinant, a.cross(point) / determinant];
            for axis in 0..2 {
                if !coordinate[axis].is_finite() {
                    return Err(diagnostic(
                        "copy_coordinate_overflow",
                        "inverse lattice coordinates are not finite",
                    ));
                }
                minimum[axis] = minimum[axis].min(coordinate[axis]);
                maximum[axis] = maximum[axis].max(coordinate[axis]);
            }
        }
        let lower = [expanded_floor(minimum[0])?, expanded_floor(minimum[1])?];
        let upper = [expanded_ceil(maximum[0])?, expanded_ceil(maximum[1])?];
        let count_a = i64::from(upper[0])
            .checked_sub(i64::from(lower[0]))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| diagnostic("copy_coordinate_overflow", "copy range overflow"))?;
        let count_b = i64::from(upper[1])
            .checked_sub(i64::from(lower[1]))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| diagnostic("copy_coordinate_overflow", "copy range overflow"))?;
        let candidate_count = usize::try_from(count_a)
            .ok()
            .and_then(|left| {
                usize::try_from(count_b)
                    .ok()
                    .and_then(|right| left.checked_mul(right))
            })
            .ok_or_else(|| {
                diagnostic("budget_candidate_copies", "candidate copy count overflow")
            })?;
        if candidate_count > budget.max_candidate_copies {
            return Err(diagnostic(
                "budget_candidate_copies",
                format!(
                    "candidate copy count {candidate_count} exceeds budget {}",
                    budget.max_candidate_copies
                ),
            ));
        }
        Ok(Self {
            lower,
            upper,
            candidate_count,
        })
    }

    pub fn contains(&self, offset: [i32; 2]) -> bool {
        (self.lower[0]..=self.upper[0]).contains(&offset[0])
            && (self.lower[1]..=self.upper[1]).contains(&offset[1])
    }

    pub fn candidate_count(&self) -> usize {
        self.candidate_count
    }

    pub fn iter(&self) -> impl Iterator<Item = [i32; 2]> + '_ {
        (self.lower[0]..=self.upper[0])
            .flat_map(move |a| (self.lower[1]..=self.upper[1]).map(move |b| [a, b]))
    }
}

fn expanded_floor(value: f64) -> Result<i32, TilingDiagnostic> {
    checked_coordinate(value.floor() - 1.0)
}

fn expanded_ceil(value: f64) -> Result<i32, TilingDiagnostic> {
    checked_coordinate(value.ceil() + 1.0)
}

fn checked_coordinate(value: f64) -> Result<i32, TilingDiagnostic> {
    if !value.is_finite() || value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        Err(diagnostic(
            "copy_coordinate_overflow",
            "required lattice copy offset exceeds the supported integer range",
        ))
    } else {
        Ok(value as i32)
    }
}

fn diagnostic(code: &'static str, message: impl Into<String>) -> TilingDiagnostic {
    TilingDiagnostic {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_at(min: [f64; 2], max: [f64; 2]) -> Aabb {
        Aabb::new(Vec2::new(min[0], min[1]), Vec2::new(max[0], max[1])).unwrap()
    }

    #[test]
    fn bounds_find_required_copy_beyond_fixed_neighborhoods() {
        let bounds = LatticeCopyBounds::for_aabb(
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 1.0),
            box_at([0.0, 0.0], [0.25, 0.25]),
            box_at([10.1, -4.1], [10.4, -3.8]),
            GeometryBudget::authoritative(),
        )
        .unwrap();
        assert!(bounds.contains([10, -4]));
    }

    #[test]
    fn inverse_lattice_bounds_cover_brute_force_oracle_at_extreme_scales() {
        for scale in [1e-6, 1.0, 1e6] {
            let a = Vec2::new(1.0 * scale, 0.2 * scale);
            let b = Vec2::new(0.35 * scale, 0.9 * scale);
            let source = box_at([-0.45 * scale, -0.3 * scale], [0.2 * scale, 0.4 * scale]);
            let target = box_at([-1.1 * scale, -0.8 * scale], [1.4 * scale, 1.2 * scale]);
            let bounds =
                LatticeCopyBounds::for_aabb(a, b, source, target, GeometryBudget::authoritative())
                    .unwrap();
            for i in -8..=8 {
                for j in -8..=8 {
                    let shift = a * f64::from(i) + b * f64::from(j);
                    if source.translated(shift).intersects(target) {
                        assert!(bounds.contains([i, j]), "missed [{i},{j}] at scale {scale}");
                    }
                }
            }
        }
    }

    #[test]
    fn excessive_candidates_fail_before_iteration_or_allocation() {
        let error = LatticeCopyBounds::for_aabb(
            Vec2::new(1e-6, 0.0),
            Vec2::new(0.0, 1e-6),
            box_at([0.0, 0.0], [1.0, 1.0]),
            box_at([0.0, 0.0], [1.0, 1.0]),
            GeometryBudget::interactive(),
        )
        .unwrap_err();
        assert_eq!(error.code, "budget_candidate_copies");
    }
}
