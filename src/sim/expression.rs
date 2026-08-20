use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpressionVariable {
    X,
    Y,
    Radius,
    Distance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnaryOp {
    Neg,
    Sqrt,
    Abs,
    Exp,
    Sin,
    Cos,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Function {
    Min,
    Max,
    Clamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum KernelExpression {
    Constant(f32),
    Parameter(String),
    Variable(ExpressionVariable),
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
    #[error("clamp lower bound is greater than its upper bound")]
    InvalidClampBounds,
    #[error("kernel expression exceeded the maximum depth of 256 nodes")]
    DepthLimitExceeded,
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

/// Evaluates `expression` against `context`.
///
/// The root node is depth zero and expressions with more than 256 nested nodes
/// are rejected with [`KernelExpressionError::DepthLimitExceeded`].
pub fn evaluate(
    expression: &KernelExpression,
    context: &ExpressionContext<'_>,
) -> Result<f32, KernelExpressionError> {
    evaluate_at_depth(expression, context, 0)
}

fn evaluate_at_depth(
    expression: &KernelExpression,
    context: &ExpressionContext<'_>,
    depth: usize,
) -> Result<f32, KernelExpressionError> {
    if depth >= MAX_EXPRESSION_DEPTH {
        return Err(KernelExpressionError::DepthLimitExceeded);
    }

    let value = match expression {
        KernelExpression::Constant(value) => *value,
        KernelExpression::Parameter(name) => context
            .parameters
            .get(name)
            .copied()
            .ok_or_else(|| KernelExpressionError::MissingParameter(name.clone()))?,
        KernelExpression::Variable(kind) => match kind {
            ExpressionVariable::X => context.x,
            ExpressionVariable::Y => context.y,
            ExpressionVariable::Radius => context.radius,
            ExpressionVariable::Distance => context.distance,
        },
        KernelExpression::Binary { op, lhs, rhs } => {
            let lhs = evaluate_at_depth(lhs, context, depth + 1)?;
            let rhs = evaluate_at_depth(rhs, context, depth + 1)?;
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
            let operand = evaluate_at_depth(operand, context, depth + 1)?;
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
                .map(|argument| evaluate_at_depth(argument, context, depth + 1))
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
                    if low > high {
                        return Err(KernelExpressionError::InvalidClampBounds);
                    }
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

const MAX_EXPRESSION_DEPTH: usize = 256;

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
    fn ast_surface_is_serde_derivable() {
        fn assert_serde<T: serde::Serialize + serde::de::DeserializeOwned>() {}

        assert_serde::<KernelExpression>();
        assert_serde::<ExpressionVariable>();
        assert_serde::<BinaryOp>();
        assert_serde::<UnaryOp>();
        assert_serde::<Function>();
    }

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
                    lhs: Box::new(KernelExpression::Variable(ExpressionVariable::Distance)),
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
            evaluate(&KernelExpression::Variable(ExpressionVariable::X), &context,).unwrap(),
            3.0
        );
        assert_eq!(
            evaluate(&KernelExpression::Variable(ExpressionVariable::Y), &context,).unwrap(),
            -4.0
        );
        assert_eq!(
            evaluate(
                &KernelExpression::Variable(ExpressionVariable::Radius),
                &context,
            )
            .unwrap(),
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
    fn rejects_each_non_finite_input_or_operation() {
        let mut parameters = BTreeMap::new();
        parameters.insert("invalid".to_string(), f32::NAN);
        let context = ExpressionContext {
            x: f32::NAN,
            y: 0.0,
            radius: 0.0,
            distance: 0.0,
            parameters: &parameters,
        };
        let non_finite_constant = KernelExpression::Constant(f32::NAN);
        let non_finite_parameter = KernelExpression::Parameter("invalid".into());
        let non_finite_geometry = KernelExpression::Variable(ExpressionVariable::X);
        let exponential_overflow = KernelExpression::Unary {
            op: UnaryOp::Exp,
            operand: Box::new(KernelExpression::Constant(f32::MAX)),
        };
        let nan_power = KernelExpression::Binary {
            op: BinaryOp::Power,
            lhs: Box::new(KernelExpression::Constant(-1.0)),
            rhs: Box::new(KernelExpression::Constant(0.5)),
        };

        for expression in [
            non_finite_constant,
            non_finite_parameter,
            non_finite_geometry,
            exponential_overflow,
            nan_power,
        ] {
            assert!(matches!(
                evaluate(&expression, &context),
                Err(KernelExpressionError::NonFinite)
            ));
        }
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

    #[test]
    fn rejects_inverted_clamp_bounds_without_panicking() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };
        let expression = KernelExpression::Call {
            function: Function::Clamp,
            arguments: vec![
                KernelExpression::Constant(0.0),
                KernelExpression::Constant(1.0),
                KernelExpression::Constant(0.0),
            ],
        };

        let result = std::panic::catch_unwind(|| evaluate(&expression, &context));
        assert!(matches!(
            result,
            Ok(Err(KernelExpressionError::InvalidClampBounds))
        ));
    }

    #[test]
    fn rejects_expressions_deeper_than_the_documented_limit() {
        let parameters = BTreeMap::new();
        let context = ExpressionContext {
            x: 0.0,
            y: 0.0,
            radius: 1.0,
            distance: 0.0,
            parameters: &parameters,
        };
        let nested_negation = |count| {
            (0..count).fold(KernelExpression::Constant(1.0), |operand, _| {
                KernelExpression::Unary {
                    op: UnaryOp::Neg,
                    operand: Box::new(operand),
                }
            })
        };
        // Each expression includes the constant leaf, so these are 256 and 257
        // total AST nodes respectively.
        let maximum = nested_negation(255);
        let too_deep = nested_negation(256);

        assert!(evaluate(&maximum, &context).is_ok());
        assert!(matches!(
            evaluate(&too_deep, &context),
            Err(KernelExpressionError::DepthLimitExceeded)
        ));
    }
}
