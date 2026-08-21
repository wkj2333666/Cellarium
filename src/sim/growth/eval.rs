use super::ast::{BinaryOp, UnaryOp};
use super::types::{SymbolId, TypedExpr, TypedExprKind, TypedProgram};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct ScalarInputs {
    pub kernel_inputs: Vec<f32>,
    pub self_value: f32,
    pub parameters: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TraceBinding {
    pub name: String,
    pub value: f32,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct EvalTrace {
    pub bindings: Vec<TraceBinding>,
    pub selected_branches: Vec<bool>,
    pub result: f32,
}
impl EvalTrace {
    pub fn binding(&self, name: &str) -> Option<f32> {
        self.bindings
            .iter()
            .rev()
            .find(|binding| binding.name == name)
            .map(|binding| binding.value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("division by zero")]
    DivideByZero,
    #[error("square root or logarithm domain error")]
    Domain,
    #[error("invalid numeric result")]
    NonFinite,
    #[error("invalid function arguments")]
    InvalidArguments,
    #[error("missing external value")]
    MissingInput,
}

pub fn evaluate(program: &TypedProgram, inputs: &ScalarInputs) -> Result<f32, EvalError> {
    Ok(evaluate_with_trace(program, inputs)?.result)
}

pub fn evaluate_with_trace(
    program: &TypedProgram,
    inputs: &ScalarInputs,
) -> Result<EvalTrace, EvalError> {
    let mut values =
        vec![Value::Scalar(0.0); program.symbol_count.max(program.externals.ordered().len())];
    for (index, name) in program.externals.ordered().iter().enumerate() {
        let value = if index < program.externals.kernel_inputs.len() {
            *inputs
                .kernel_inputs
                .get(index)
                .ok_or(EvalError::MissingInput)?
        } else if name == "self" {
            inputs.self_value
        } else {
            *inputs.parameters.get(name).ok_or(EvalError::MissingInput)?
        };
        values[index] = Value::Scalar(value);
    }
    let mut trace = EvalTrace::default();
    for binding in &program.bindings {
        let value = eval_expr(&binding.value, &mut values, &mut trace)?;
        assign(binding.id, value, &mut values);
        if let Value::Scalar(value) = value {
            trace.bindings.push(TraceBinding {
                name: binding.name.clone(),
                value,
            });
        }
    }
    let result = eval_expr(&program.result, &mut values, &mut trace)?;
    let result = match result {
        Value::Scalar(value) => value,
        Value::Bool(_) => return Err(EvalError::InvalidArguments),
    };
    trace.result = finite(result)?;
    Ok(trace)
}

#[derive(Clone, Copy)]
enum Value {
    Scalar(f32),
    Bool(bool),
}
fn assign(id: SymbolId, value: Value, values: &mut Vec<Value>) {
    let index = id.0 as usize;
    if index >= values.len() {
        values.resize(index + 1, Value::Scalar(0.0));
    }
    values[index] = value;
}
fn get(id: SymbolId, values: &[Value]) -> Value {
    values
        .get(id.0 as usize)
        .copied()
        .unwrap_or(Value::Scalar(0.0))
}
fn scalar(value: Value) -> Result<f32, EvalError> {
    match value {
        Value::Scalar(value) => finite(value),
        Value::Bool(_) => Err(EvalError::InvalidArguments),
    }
}
fn boolean(value: Value) -> Result<bool, EvalError> {
    match value {
        Value::Bool(value) => Ok(value),
        Value::Scalar(_) => Err(EvalError::InvalidArguments),
    }
}
fn finite(value: f32) -> Result<f32, EvalError> {
    value
        .is_finite()
        .then_some(value)
        .ok_or(EvalError::NonFinite)
}

fn eval_expr(
    expr: &TypedExpr,
    values: &mut Vec<Value>,
    trace: &mut EvalTrace,
) -> Result<Value, EvalError> {
    match &expr.kind {
        TypedExprKind::Constant(value) => Ok(Value::Scalar(*value)),
        TypedExprKind::Bool(value) => Ok(Value::Bool(*value)),
        TypedExprKind::Symbol(id) => Ok(get(*id, values)),
        TypedExprKind::Unary { op, operand } => match op {
            UnaryOp::Neg => Ok(Value::Scalar(finite(-scalar(eval_expr(
                operand, values, trace,
            )?)?)?)),
            UnaryOp::Not => Ok(Value::Bool(!boolean(eval_expr(operand, values, trace)?)?)),
        },
        TypedExprKind::Binary { op, lhs, rhs } => match op {
            BinaryOp::And => {
                let left = boolean(eval_expr(lhs, values, trace)?)?;
                if !left {
                    Ok(Value::Bool(false))
                } else {
                    Ok(Value::Bool(boolean(eval_expr(rhs, values, trace)?)?))
                }
            }
            BinaryOp::Or => {
                let left = boolean(eval_expr(lhs, values, trace)?)?;
                if left {
                    Ok(Value::Bool(true))
                } else {
                    Ok(Value::Bool(boolean(eval_expr(rhs, values, trace)?)?))
                }
            }
            BinaryOp::Equal
            | BinaryOp::NotEqual
            | BinaryOp::Less
            | BinaryOp::LessEqual
            | BinaryOp::Greater
            | BinaryOp::GreaterEqual => {
                let left = scalar(eval_expr(lhs, values, trace)?)?;
                let right = scalar(eval_expr(rhs, values, trace)?)?;
                let result = match op {
                    BinaryOp::Equal => left == right,
                    BinaryOp::NotEqual => left != right,
                    BinaryOp::Less => left < right,
                    BinaryOp::LessEqual => left <= right,
                    BinaryOp::Greater => left > right,
                    BinaryOp::GreaterEqual => left >= right,
                    _ => unreachable!(),
                };
                Ok(Value::Bool(result))
            }
            _ => {
                let left = scalar(eval_expr(lhs, values, trace)?)?;
                let right = scalar(eval_expr(rhs, values, trace)?)?;
                let value = match op {
                    BinaryOp::Add => left + right,
                    BinaryOp::Subtract => left - right,
                    BinaryOp::Multiply => left * right,
                    BinaryOp::Divide => {
                        if right == 0.0 {
                            return Err(EvalError::DivideByZero);
                        } else {
                            left / right
                        }
                    }
                    BinaryOp::Power => left.powf(right),
                    _ => unreachable!(),
                };
                Ok(Value::Scalar(finite(value)?))
            }
        },
        TypedExprKind::Call { name, arguments } => {
            let values = arguments
                .iter()
                .map(|argument| scalar(eval_expr(argument, values, trace)?))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Value::Scalar(call(name, &values)?))
        }
        TypedExprKind::If {
            condition,
            then_branch,
            then_result,
            else_branch,
            else_result,
        } => {
            let selected = boolean(eval_expr(condition, values, trace)?)?;
            trace.selected_branches.push(selected);
            let bindings = if selected { then_branch } else { else_branch };
            for binding in bindings {
                let value = eval_expr(&binding.value, values, trace)?;
                assign(binding.id, value, values);
                if let Value::Scalar(value) = value {
                    trace.bindings.push(TraceBinding {
                        name: binding.name.clone(),
                        value,
                    });
                }
            }
            if selected {
                eval_expr(then_result, values, trace)
            } else {
                eval_expr(else_result, values, trace)
            }
        }
    }
}

fn call(name: &str, values: &[f32]) -> Result<f32, EvalError> {
    let value = match name {
        "sqrt" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .sqrt(),
        "abs" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .abs(),
        "exp" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .exp(),
        "log" => {
            let value = values.first().copied().ok_or(EvalError::InvalidArguments)?;
            if value <= 0.0 {
                return Err(EvalError::Domain);
            }
            value.ln()
        }
        "sin" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .sin(),
        "cos" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .cos(),
        "tanh" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .tanh(),
        "floor" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .floor(),
        "ceil" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .ceil(),
        "round" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .round(),
        "sign" => values
            .first()
            .copied()
            .ok_or(EvalError::InvalidArguments)?
            .signum(),
        "min" => values
            .first()
            .copied()
            .zip(values.get(1).copied())
            .map(|(a, b)| a.min(b))
            .ok_or(EvalError::InvalidArguments)?,
        "max" => values
            .first()
            .copied()
            .zip(values.get(1).copied())
            .map(|(a, b)| a.max(b))
            .ok_or(EvalError::InvalidArguments)?,
        "step" => values
            .first()
            .copied()
            .zip(values.get(1).copied())
            .map(|(edge, x)| if x < edge { 0.0 } else { 1.0 })
            .ok_or(EvalError::InvalidArguments)?,
        "clamp" => {
            if values.len() == 3 {
                values[0].clamp(values[1], values[2])
            } else {
                return Err(EvalError::InvalidArguments);
            }
        }
        "smoothstep" => {
            if values.len() == 3 {
                let t = ((values[2] - values[0]) / (values[1] - values[0])).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            } else {
                return Err(EvalError::InvalidArguments);
            }
        }
        "mix" => {
            if values.len() == 3 {
                values[0] + (values[1] - values[0]) * values[2]
            } else {
                return Err(EvalError::InvalidArguments);
            }
        }
        "gauss" if values.len() == 3 && values[2] != 0.0 => {
            (-0.5 * ((values[0] - values[1]) / values[2]).powi(2)).exp()
        }
        _ => return Err(EvalError::InvalidArguments),
    };
    finite(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::growth::typecheck::compile;
    use crate::sim::growth::types::ExternalSymbols;
    fn eval(source: &str) -> Result<f32, EvalError> {
        let program = compile(source, &ExternalSymbols::new(&["inner"], &[])).unwrap();
        evaluate(
            &program,
            &ScalarInputs {
                kernel_inputs: vec![0.4],
                self_value: 0.4,
                parameters: BTreeMap::new(),
            },
        )
    }
    #[test]
    fn if_and_boolean_operators_short_circuit_invalid_math() {
        assert_eq!(eval("if false { sqrt(-1.0) } else { 0.25 }").unwrap(), 0.25);
        assert_eq!(
            eval("if false && (1.0 / 0.0 > 0.0) { 1.0 } else { 0.0 }").unwrap(),
            0.0
        );
        assert_eq!(
            eval("if true || (1.0 / 0.0 > 0.0) { 1.0 } else { 0.0 }").unwrap(),
            1.0
        );
    }
    #[test]
    fn trace_reports_locals_branch_and_result() {
        let p = compile(
            "let x = inner * 2.0; if x > 0.5 { x } else { 0.0 }",
            &ExternalSymbols::new(&["inner"], &[]),
        )
        .unwrap();
        let trace = evaluate_with_trace(
            &p,
            &ScalarInputs {
                kernel_inputs: vec![0.4],
                self_value: 0.4,
                parameters: BTreeMap::new(),
            },
        )
        .unwrap();
        assert_eq!(trace.binding("x"), Some(0.8));
        assert_eq!(trace.selected_branches.len(), 1);
        assert_eq!(trace.result, 0.8);
    }
}
