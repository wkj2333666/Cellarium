use std::collections::BTreeMap;

use super::{
    BasisId, EdgeRef, PeriodicTilingDraft, PrototypeId, PrototypeShape, SeamConstraint, Vec2,
    polygon::{prototype_vertices, validate_polygon},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragTarget {
    pub prototype: PrototypeId,
    pub vertex: usize,
    pub to: Vec2,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SolvedTiling {
    pub draft: PeriodicTilingDraft,
    pub max_displacement: f64,
    pub max_seam_residual: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolveDiagnostic(pub String);

pub fn solve_edge_constraints(
    draft: &PeriodicTilingDraft,
    constraints: &[SeamConstraint],
    drag_target: Option<DragTarget>,
) -> Result<SolvedTiling, SolveDiagnostic> {
    if constraints.is_empty() {
        return Err(diagnostic("select at least one complete edge pair"));
    }
    if constraints.len() > 1_024 {
        return Err(diagnostic("edge constraint count exceeds the solve budget"));
    }

    let mut values = Vec::new();
    let mut prototype_vars = BTreeMap::new();
    for prototype in &draft.prototypes {
        let vertices = prototype_vertices(&prototype.shape)
            .map_err(|issues| diagnostic(join_issues(issues)))?;
        let start = values.len();
        for vertex in &vertices {
            values.extend([vertex.x, vertex.y]);
        }
        prototype_vars.insert(prototype.id, (start, vertices.len()));
    }
    let mut instance_vars = BTreeMap::new();
    for instance in &draft.instances {
        let start = values.len();
        values.extend([
            instance.transform.translation.x,
            instance.transform.translation.y,
        ]);
        instance_vars.insert(
            instance.id,
            (start, instance.prototype, instance.transform.rotation),
        );
    }
    let lattice_start = values.len();
    values.extend([
        draft.translation_a.x,
        draft.translation_a.y,
        draft.translation_b.x,
        draft.translation_b.y,
    ]);
    if values.len() > 8_192 {
        return Err(diagnostic("tiling variable count exceeds the solve budget"));
    }
    let original = values.clone();
    let mut inverse_weights = vec![1.0; values.len()];
    if let Some(target) = drag_target {
        let (start, count) = prototype_vars
            .get(&target.prototype)
            .copied()
            .ok_or_else(|| diagnostic("drag target prototype is missing"))?;
        if target.vertex >= count || !target.to.x.is_finite() || !target.to.y.is_finite() {
            return Err(diagnostic("drag target vertex is invalid"));
        }
        values[start + target.vertex * 2] = target.to.x;
        values[start + target.vertex * 2 + 1] = target.to.y;
        inverse_weights[start + target.vertex * 2] = 1e-4;
        inverse_weights[start + target.vertex * 2 + 1] = 1e-4;
    }

    let mut rows = Vec::with_capacity(constraints.len() * 4);
    for constraint in constraints {
        append_endpoint_rows(
            &mut rows,
            values.len(),
            draft,
            *constraint,
            true,
            &prototype_vars,
            &instance_vars,
            lattice_start,
        )?;
        append_endpoint_rows(
            &mut rows,
            values.len(),
            draft,
            *constraint,
            false,
            &prototype_vars,
            &instance_vars,
            lattice_start,
        )?;
    }
    project_to_constraints(&mut values, &rows, &inverse_weights)?;
    if values.iter().any(|value| !value.is_finite()) {
        return Err(diagnostic("constraint solve produced non-finite geometry"));
    }

    let mut solved = draft.clone();
    for prototype in &mut solved.prototypes {
        let (start, count) = prototype_vars[&prototype.id];
        let vertices = (0..count)
            .map(|vertex| Vec2::new(values[start + vertex * 2], values[start + vertex * 2 + 1]))
            .collect::<Vec<_>>();
        if let Some(issue) = validate_polygon(&vertices).into_iter().next() {
            return Err(diagnostic(format!(
                "solved polygon {} is invalid: {}",
                prototype.id.0, issue.message
            )));
        }
        prototype.shape = PrototypeShape::SimplePolygon { vertices };
    }
    for instance in &mut solved.instances {
        let (start, _, _) = instance_vars[&instance.id];
        instance.transform.translation = Vec2::new(values[start], values[start + 1]);
    }
    solved.translation_a = Vec2::new(values[lattice_start], values[lattice_start + 1]);
    solved.translation_b = Vec2::new(values[lattice_start + 2], values[lattice_start + 3]);

    let max_seam_residual = constraint_residual(&rows, &values);
    if max_seam_residual > solve_tolerance(&values) {
        return Err(diagnostic(format!(
            "edge constraints remain inconsistent (residual {max_seam_residual:.3e})"
        )));
    }
    crate::sim::tiling::validate_coverage(&solved).map_err(|issues| {
        diagnostic(format!(
            "solved seams do not form an exact periodic tiling: {}",
            issues
                .into_iter()
                .map(|issue| issue.message)
                .collect::<Vec<_>>()
                .join("; ")
        ))
    })?;
    let max_displacement = values
        .iter()
        .zip(original)
        .map(|(after, before)| (after - before).abs())
        .fold(0.0_f64, f64::max);
    Ok(SolvedTiling {
        draft: solved,
        max_displacement,
        max_seam_residual,
    })
}

type PrototypeVars = BTreeMap<PrototypeId, (usize, usize)>;
type InstanceVars = BTreeMap<BasisId, (usize, PrototypeId, f64)>;

#[allow(clippy::too_many_arguments)]
fn append_endpoint_rows(
    rows: &mut Vec<Vec<f64>>,
    variable_count: usize,
    draft: &PeriodicTilingDraft,
    constraint: SeamConstraint,
    first_endpoint: bool,
    prototype_vars: &PrototypeVars,
    instance_vars: &InstanceVars,
    lattice_start: usize,
) -> Result<(), SolveDiagnostic> {
    let lhs_vertex = if first_endpoint {
        usize::from(constraint.lhs.edge)
    } else {
        usize::from(constraint.lhs.edge) + 1
    };
    let rhs_vertex = if first_endpoint {
        usize::from(constraint.rhs.edge) + 1
    } else {
        usize::from(constraint.rhs.edge)
    };
    for component in 0..2 {
        let mut row = vec![0.0; variable_count];
        add_world_vertex(
            &mut row,
            draft,
            constraint.lhs,
            lhs_vertex,
            component,
            1.0,
            prototype_vars,
            instance_vars,
        )?;
        add_world_vertex(
            &mut row,
            draft,
            constraint.rhs,
            rhs_vertex,
            component,
            -1.0,
            prototype_vars,
            instance_vars,
        )?;
        row[lattice_start + component] -= f64::from(constraint.periodic_offset[0]);
        row[lattice_start + 2 + component] -= f64::from(constraint.periodic_offset[1]);
        rows.push(row);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add_world_vertex(
    row: &mut [f64],
    draft: &PeriodicTilingDraft,
    edge: EdgeRef,
    vertex: usize,
    component: usize,
    sign: f64,
    prototype_vars: &PrototypeVars,
    instance_vars: &InstanceVars,
) -> Result<(), SolveDiagnostic> {
    let (translation, prototype, rotation) = instance_vars
        .get(&edge.tile)
        .copied()
        .ok_or_else(|| diagnostic("constraint references a missing basis"))?;
    let (prototype_start, count) = prototype_vars
        .get(&prototype)
        .copied()
        .ok_or_else(|| diagnostic("constraint references a missing prototype"))?;
    let vertex = vertex % count;
    let (sine, cosine) = rotation.sin_cos();
    let coefficients = if component == 0 {
        [cosine, -sine]
    } else {
        [sine, cosine]
    };
    row[prototype_start + vertex * 2] += sign * coefficients[0];
    row[prototype_start + vertex * 2 + 1] += sign * coefficients[1];
    row[translation + component] += sign;
    let _ = draft;
    Ok(())
}

fn project_to_constraints(
    values: &mut [f64],
    rows: &[Vec<f64>],
    inverse_weights: &[f64],
) -> Result<(), SolveDiagnostic> {
    let count = rows.len();
    let mut gram = vec![vec![0.0; count]; count];
    for left in 0..count {
        for right in left..count {
            let value = rows[left]
                .iter()
                .zip(&rows[right])
                .zip(inverse_weights)
                .map(|((lhs, rhs), weight)| lhs * rhs * weight)
                .sum::<f64>();
            gram[left][right] = value;
            gram[right][left] = value;
        }
    }
    let diagonal_scale = (0..count)
        .map(|index| gram[index][index].abs())
        .fold(1.0_f64, f64::max);
    for (index, row) in gram.iter_mut().enumerate() {
        row[index] += diagonal_scale * 1e-13;
    }
    for _ in 0..4 {
        let residual = rows.iter().map(|row| dot(row, values)).collect::<Vec<_>>();
        if residual.iter().map(|value| value.abs()).fold(0.0, f64::max) <= solve_tolerance(values) {
            return Ok(());
        }
        let multipliers = solve_linear(gram.clone(), residual)?;
        for variable in 0..values.len() {
            let correction = rows
                .iter()
                .zip(&multipliers)
                .map(|(row, multiplier)| row[variable] * multiplier)
                .sum::<f64>()
                * inverse_weights[variable];
            values[variable] -= correction;
        }
    }
    Ok(())
}

fn solve_linear(mut matrix: Vec<Vec<f64>>, mut rhs: Vec<f64>) -> Result<Vec<f64>, SolveDiagnostic> {
    let count = rhs.len();
    for column in 0..count {
        let pivot = (column..count)
            .max_by(|left, right| {
                matrix[*left][column]
                    .abs()
                    .total_cmp(&matrix[*right][column].abs())
            })
            .ok_or_else(|| diagnostic("empty constraint system"))?;
        if matrix[pivot][column].abs() <= 1e-18 {
            return Err(diagnostic("edge constraints are rank deficient"));
        }
        matrix.swap(column, pivot);
        rhs.swap(column, pivot);
        let divisor = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= divisor;
        }
        rhs[column] /= divisor;
        let pivot_row = matrix[column].clone();
        for row in 0..count {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for (value, pivot_value) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *value -= factor * pivot_value;
            }
            rhs[row] -= factor * rhs[column];
        }
    }
    Ok(rhs)
}

fn constraint_residual(rows: &[Vec<f64>], values: &[f64]) -> f64 {
    rows.iter()
        .map(|row| dot(row, values).abs())
        .fold(0.0, f64::max)
}

fn dot(left: &[f64], right: &[f64]) -> f64 {
    left.iter().zip(right).map(|(a, b)| a * b).sum()
}

fn solve_tolerance(values: &[f64]) -> f64 {
    values
        .iter()
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max)
        * 1e-9
}

fn join_issues(issues: Vec<super::GeometryIssue>) -> String {
    issues
        .into_iter()
        .map(|issue| issue.message)
        .collect::<Vec<_>>()
        .join("; ")
}

fn diagnostic(message: impl Into<String>) -> SolveDiagnostic {
    SolveDiagnostic(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::tiling::{TilingPreset, build_preset, propose_full_edge_seams};

    #[test]
    fn seam_solve_keeps_a_square_exact_and_valid() {
        let mut draft = build_preset(TilingPreset::Square, 1.0);
        let constraints = propose_full_edge_seams(&draft, 1e-6)
            .unwrap()
            .into_iter()
            .map(|proposal| proposal.constraint)
            .collect::<Vec<_>>();
        let solved = solve_edge_constraints(&draft, &constraints, None).unwrap();
        assert!(crate::sim::tiling::validate_coverage(&solved.draft).is_ok());
        assert!(solved.max_seam_residual <= 1e-9);

        draft.translation_a.x += 0.02;
        let solved = solve_edge_constraints(&draft, &constraints, None).unwrap();
        assert!(crate::sim::tiling::validate_coverage(&solved.draft).is_ok());
    }
}
