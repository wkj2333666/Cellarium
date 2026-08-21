use super::ast::BinaryOp;
use super::types::{SymbolId, TypedExpr, TypedExprKind, TypedProgram};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub low: f32,
    pub high: f32,
}
impl Interval {
    pub fn new(low: f32, high: f32) -> Self {
        Self { low, high }
    }
    pub fn union(self, other: Self) -> Self {
        Self::new(self.low.min(other.low), self.high.max(other.high))
    }
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct InputRanges(pub BTreeMap<String, Interval>);
impl InputRanges {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn with(mut self, name: impl Into<String>, range: Interval) -> Self {
        self.0.insert(name.into(), range);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Error,
}
#[derive(Clone, Debug, PartialEq)]
pub struct NumericHazard {
    pub severity: Severity,
    pub code: &'static str,
}

pub fn analyze_intervals(program: &TypedProgram, ranges: &InputRanges) -> Vec<NumericHazard> {
    let mut values = BTreeMap::new();
    for (index, name) in program.externals.ordered().iter().enumerate() {
        values.insert(
            SymbolId(index as u32),
            ranges
                .0
                .get(name)
                .copied()
                .unwrap_or(Interval::new(-1.0, 1.0)),
        );
    }
    let mut hazards = Vec::new();
    for binding in &program.bindings {
        let value = analyze_expr(&binding.value, &values, &mut hazards);
        values.insert(binding.id, value);
    }
    let _ = analyze_expr(&program.result, &values, &mut hazards);
    hazards
}

fn analyze_expr(
    expr: &TypedExpr,
    values: &BTreeMap<SymbolId, Interval>,
    hazards: &mut Vec<NumericHazard>,
) -> Interval {
    match &expr.kind {
        TypedExprKind::Constant(value) => Interval::new(*value, *value),
        TypedExprKind::Bool(_) => Interval::new(0.0, 1.0),
        TypedExprKind::Symbol(id) => values.get(id).copied().unwrap_or(Interval::new(-1.0, 1.0)),
        TypedExprKind::Unary { operand, .. } => analyze_expr(operand, values, hazards),
        TypedExprKind::Binary { op, lhs, rhs } => {
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                let left = analyze_expr(lhs, values, hazards);
                if left.low == left.high
                    && ((matches!(op, BinaryOp::And) && left.low == 0.0)
                        || (matches!(op, BinaryOp::Or) && left.low == 1.0))
                {
                    return Interval::new(0.0, 1.0);
                }
            }
            let left = analyze_expr(lhs, values, hazards);
            let right = analyze_expr(rhs, values, hazards);
            match op {
                BinaryOp::Add => Interval::new(left.low + right.low, left.high + right.high),
                BinaryOp::Subtract => Interval::new(left.low - right.high, left.high - right.low),
                BinaryOp::Multiply => {
                    let values = [
                        left.low * right.low,
                        left.low * right.high,
                        left.high * right.low,
                        left.high * right.high,
                    ];
                    Interval::new(
                        values.iter().copied().fold(f32::INFINITY, f32::min),
                        values.iter().copied().fold(f32::NEG_INFINITY, f32::max),
                    )
                }
                BinaryOp::Divide => {
                    if right.low <= 0.0 && right.high >= 0.0 {
                        hazards.push(NumericHazard {
                            severity: if right.low == 0.0 && right.high == 0.0 {
                                Severity::Error
                            } else {
                                Severity::Warning
                            },
                            code: "division_by_zero",
                        });
                        Interval::new(f32::NEG_INFINITY, f32::INFINITY)
                    } else {
                        Interval::new(left.low / right.high, left.high / right.low)
                    }
                }
                _ => Interval::new(0.0, 1.0),
            }
        }
        TypedExprKind::Call { name, arguments } => {
            let argument = arguments
                .first()
                .map(|argument| analyze_expr(argument, values, hazards))
                .unwrap_or(Interval::new(-1.0, 1.0));
            match name.as_str() {
                "sqrt" => {
                    if argument.high < 0.0 {
                        hazards.push(NumericHazard {
                            severity: Severity::Error,
                            code: "sqrt_domain",
                        });
                    } else if argument.low < 0.0 {
                        hazards.push(NumericHazard {
                            severity: Severity::Warning,
                            code: "sqrt_domain",
                        });
                    }
                    Interval::new(0.0, argument.high.max(0.0).sqrt())
                }
                "log" => {
                    if argument.high <= 0.0 {
                        hazards.push(NumericHazard {
                            severity: Severity::Error,
                            code: "log_domain",
                        });
                    } else if argument.low <= 0.0 {
                        hazards.push(NumericHazard {
                            severity: Severity::Warning,
                            code: "log_domain",
                        });
                    }
                    Interval::new(f32::NEG_INFINITY, f32::INFINITY)
                }
                _ => Interval::new(f32::NEG_INFINITY, f32::INFINITY),
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            then_result,
            else_branch,
            else_result,
        } => {
            if let TypedExprKind::Bool(value) = condition.kind {
                return if value {
                    analyze_block(then_branch, then_result, values, hazards)
                } else {
                    analyze_block(else_branch, else_result, values, hazards)
                };
            }
            let condition = analyze_expr(condition, values, hazards);
            if condition.low == 0.0 && condition.high == 0.0 {
                return analyze_block(else_branch, else_result, values, hazards);
            }
            if condition.low == 1.0 && condition.high == 1.0 {
                return analyze_block(then_branch, then_result, values, hazards);
            }
            let left = analyze_block(then_branch, then_result, values, hazards);
            let right = analyze_block(else_branch, else_result, values, hazards);
            left.union(right)
        }
    }
}
fn analyze_block(
    bindings: &[super::types::TypedBinding],
    result: &TypedExpr,
    values: &BTreeMap<SymbolId, Interval>,
    hazards: &mut Vec<NumericHazard>,
) -> Interval {
    let mut local = values.clone();
    for binding in bindings {
        let value = analyze_expr(&binding.value, &local, hazards);
        local.insert(binding.id, value);
    }
    analyze_expr(result, &local, hazards)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::growth::typecheck::compile;
    use crate::sim::growth::types::ExternalSymbols;
    fn analyze(source: &str, ranges: InputRanges) -> Vec<NumericHazard> {
        let p = compile(source, &ExternalSymbols::new(&["inner"], &[])).unwrap();
        analyze_intervals(&p, &ranges)
    }
    fn range(name: &str, low: f32, high: f32) -> InputRanges {
        InputRanges::empty().with(name, Interval::new(low, high))
    }
    #[test]
    fn distinguishes_guaranteed_from_possible_invalid_domains() {
        let guaranteed = analyze("sqrt(inner)", range("inner", -2.0, -1.0));
        assert!(guaranteed.iter().any(|h| h.severity == Severity::Error));
        let possible = analyze("sqrt(inner)", range("inner", -1.0, 1.0));
        assert!(possible.iter().any(|h| h.severity == Severity::Warning));
    }
    #[test]
    fn selected_constant_branch_suppresses_unreachable_hazard() {
        assert!(analyze("if false { 1.0 / 0.0 } else { 0.0 }", InputRanges::empty()).is_empty());
    }
}
