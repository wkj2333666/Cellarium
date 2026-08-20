use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VariableKind {
    X,
    Y,
    Radius,
    Distance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Sqrt,
    Abs,
    Exp,
    Sin,
    Cos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Function {
    Min,
    Max,
    Clamp,
}

#[derive(Clone, Debug, PartialEq)]
pub enum KernelExpression {
    Constant(f32),
    Parameter(String),
    Variable(VariableKind),
    Binary {
        op: BinaryOp,
        lhs: Box<KernelExpression>,
        rhs: Box<KernelExpression>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<KernelExpression>,
    },
    Call {
        function: Function,
        arguments: Vec<KernelExpression>,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum KernelExpressionError {
    #[error("missing kernel parameter: {0}")]
    MissingParameter(String),
    #[error("kernel expression produced a non-finite value")]
    NonFinite,
    #[error("division by zero in kernel expression")]
    DivideByZero,
    #[error("square root of a negative kernel value")]
    NegativeSqrt,
    #[error("{0} requires {1} arguments, received {2}")]
    ArgumentCount(&'static str, usize, usize),
}

pub struct ExpressionContext<'a> {
    pub x: f32,
    pub y: f32,
    pub radius: f32,
    pub distance: f32,
    pub parameters: &'a BTreeMap<String, f32>,
}

pub fn evaluate(
    expression: &KernelExpression,
    context: &ExpressionContext<'_>,
) -> Result<f32, KernelExpressionError> {
    let value = match expression {
        KernelExpression::Constant(value) => *value,
        KernelExpression::Parameter(name) => context
            .parameters
            .get(name)
            .copied()
            .ok_or_else(|| KernelExpressionError::MissingParameter(name.clone()))?,
        KernelExpression::Variable(kind) => match kind {
            VariableKind::X => context.x,
            VariableKind::Y => context.y,
            VariableKind::Radius => context.radius,
            VariableKind::Distance => context.distance,
        },
        KernelExpression::Binary { op, lhs, rhs } => {
            let lhs = evaluate(lhs, context)?;
            let rhs = evaluate(rhs, context)?;
            match op {
                BinaryOp::Add => lhs + rhs,
                BinaryOp::Subtract => lhs - rhs,
                BinaryOp::Multiply => lhs * rhs,
                BinaryOp::Divide => {
                    if rhs == 0.0 {
                        return Err(KernelExpressionError::DivideByZero);
                    }
                    lhs / rhs
                }
                BinaryOp::Power => lhs.powf(rhs),
            }
        }
        KernelExpression::Unary { op, operand } => {
            let operand = evaluate(operand, context)?;
            match op {
                UnaryOp::Neg => -operand,
                UnaryOp::Sqrt => {
                    if operand < 0.0 {
                        return Err(KernelExpressionError::NegativeSqrt);
                    }
                    operand.sqrt()
                }
                UnaryOp::Abs => operand.abs(),
                UnaryOp::Exp => operand.exp(),
                UnaryOp::Sin => operand.sin(),
                UnaryOp::Cos => operand.cos(),
            }
        }
        KernelExpression::Call {
            function,
            arguments,
        } => {
            let arguments = arguments
                .iter()
                .map(|argument| evaluate(argument, context))
                .collect::<Result<Vec<_>, _>>()?;
            match function {
                Function::Min => {
                    require_arguments("min", 2, &arguments)?;
                    let [lhs, rhs] = arguments[..] else {
                        unreachable!("argument count was checked")
                    };
                    lhs.min(rhs)
                }
                Function::Max => {
                    require_arguments("max", 2, &arguments)?;
                    let [lhs, rhs] = arguments[..] else {
                        unreachable!("argument count was checked")
                    };
                    lhs.max(rhs)
                }
                Function::Clamp => {
                    require_arguments("clamp", 3, &arguments)?;
                    let [value, low, high] = arguments[..] else {
                        unreachable!("argument count was checked")
                    };
                    value.clamp(low, high)
                }
            }
        }
    };

    if value.is_finite() {
        Ok(value)
    } else {
        Err(KernelExpressionError::NonFinite)
    }
}

fn require_arguments(
    name: &'static str,
    expected: usize,
    arguments: &[f32],
) -> Result<(), KernelExpressionError> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(KernelExpressionError::ArgumentCount(
            name,
            expected,
            arguments.len(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn evaluates_geometry_parameters_and_math() {
        let mut parameters = BTreeMap::new();
        parameters.insert("center".to_string(), 0.5);
        parameters.insert("width".to_string(), 0.25);
        let context = ExpressionContext {
            x: 3.0,
            y: -4.0,
            radius: 5.0,
            distance: 1.0,
            parameters: &parameters,
        };
        let expression = KernelExpression::Unary {
            op: UnaryOp::Exp,
            operand: Box::new(KernelExpression::Binary {
                op: BinaryOp::Divide,
                lhs: Box::new(KernelExpression::Binary {
                    op: BinaryOp::Subtract,
                    lhs: Box::new(KernelExpression::Variable(VariableKind::Distance)),
                    rhs: Box::new(KernelExpression::Parameter("center".into())),
                }),
                rhs: Box::new(KernelExpression::Parameter("width".into())),
            }),
        };

        assert_eq!(evaluate(&expression, &context).unwrap(), (2.0_f32).exp());
    }

    #[test]
    fn exposes_all_geometry_variables() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 3.0,
            y: -4.0,
            radius: 5.0,
            distance: 1.0,
            parameters: &parameters,
        };

        assert_eq!(
            evaluate(&KernelExpression::Variable(VariableKind::X), &context).unwrap(),
            3.0
        );
        assert_eq!(
            evaluate(&KernelExpression::Variable(VariableKind::Y), &context).unwrap(),
            -4.0
        );
        assert_eq!(
            evaluate(&KernelExpression::Variable(VariableKind::Radius), &context).unwrap(),
            5.0
        );
    }

    #[test]
    fn rejects_missing_parameters_and_non_finite_results() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };

        let missing = KernelExpression::Parameter("missing".into());
        assert!(evaluate(&missing, &context).is_err());

        let overflow = KernelExpression::Binary {
            op: BinaryOp::Multiply,
            lhs: Box::new(KernelExpression::Constant(f32::MAX)),
            rhs: Box::new(KernelExpression::Constant(f32::MAX)),
        };
        assert!(evaluate(&overflow, &context).is_err());
    }

    #[test]
    fn rejects_invalid_unary_and_binary_operations() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };

        let division = KernelExpression::Binary {
            op: BinaryOp::Divide,
            lhs: Box::new(KernelExpression::Constant(1.0)),
            rhs: Box::new(KernelExpression::Constant(0.0)),
        };
        assert!(evaluate(&division, &context).is_err());

        let sqrt = KernelExpression::Unary {
            op: UnaryOp::Sqrt,
            operand: Box::new(KernelExpression::Constant(-1.0)),
        };
        assert!(evaluate(&sqrt, &context).is_err());
    }

    #[test]
    fn evaluates_absolute_clamp_min_max_and_power() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };

        assert_eq!(
            evaluate(
                &KernelExpression::Unary {
                    op: UnaryOp::Abs,
                    operand: Box::new(KernelExpression::Constant(-2.0)),
                },
                &context
            )
            .unwrap(),
            2.0
        );
        assert_eq!(
            evaluate(
                &KernelExpression::Call {
                    function: Function::Clamp,
                    arguments: vec![
                        KernelExpression::Constant(-1.0),
                        KernelExpression::Constant(0.0),
                        KernelExpression::Constant(1.0),
                    ],
                },
                &context
            )
            .unwrap(),
            0.0
        );
        assert_eq!(
            evaluate(
                &KernelExpression::Call {
                    function: Function::Min,
                    arguments: vec![
                        KernelExpression::Constant(2.0),
                        KernelExpression::Constant(5.0),
                    ],
                },
                &context
            )
            .unwrap(),
            2.0
        );
        assert_eq!(
            evaluate(
                &KernelExpression::Call {
                    function: Function::Max,
                    arguments: vec![
                        KernelExpression::Constant(2.0),
                        KernelExpression::Constant(5.0),
                    ],
                },
                &context
            )
            .unwrap(),
            5.0
        );
        assert_eq!(
            evaluate(
                &KernelExpression::Binary {
                    op: BinaryOp::Power,
                    lhs: Box::new(KernelExpression::Constant(2.0)),
                    rhs: Box::new(KernelExpression::Constant(3.0)),
                },
                &context
            )
            .unwrap(),
            8.0
        );
    }

    #[test]
    fn rejects_wrong_function_argument_counts() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };
        let cases = [
            (Function::Min, Vec::new(), "min", 2_usize, 0_usize),
            (
                Function::Max,
                vec![KernelExpression::Constant(0.0)],
                "max",
                2,
                1,
            ),
            (
                Function::Clamp,
                vec![
                    KernelExpression::Constant(0.0),
                    KernelExpression::Constant(1.0),
                ],
                "clamp",
                3,
                2,
            ),
        ];

        for (function, arguments, name, expected_count, received_count) in cases {
            let expression = KernelExpression::Call {
                function,
                arguments,
            };
            match evaluate(&expression, &context) {
                Err(KernelExpressionError::ArgumentCount(
                    actual_name,
                    actual_count,
                    actual_received_count,
                )) => {
                    assert_eq!(actual_name, name);
                    assert_eq!(actual_count, expected_count);
                    assert_eq!(actual_received_count, received_count);
                }
                result => panic!("expected an argument-count error, got {result:?}"),
            }
        }
    }
}
