use super::ast::{BinaryOp, Block, Expr, ExprKind, LetStatement, UnaryOp};
use super::lexer::Span;
use super::parser::parse_program;
use super::types::*;
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeDiagnostic {
    pub code: &'static str,
    pub span: Span,
}

pub fn compile(
    source: &str,
    externals: &ExternalSymbols,
) -> Result<TypedProgram, Vec<TypeDiagnostic>> {
    let program = parse_program(source).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| TypeDiagnostic {
                code: error.code,
                span: error.span,
            })
            .collect::<Vec<_>>()
    })?;
    typecheck(program, externals)
}

pub fn typecheck(
    program: super::ast::Program,
    externals: &ExternalSymbols,
) -> Result<TypedProgram, Vec<TypeDiagnostic>> {
    let names = externals.ordered();
    let mut symbols = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        symbols.insert(name.clone(), SymbolId(index as u32));
    }
    let mut next_id = names.len() as u32;
    let mut errors = Vec::new();
    let mut scope = symbols.clone();
    let bindings = check_bindings(
        &program.bindings,
        &mut scope,
        &mut next_id,
        &mut errors,
        &symbols,
    );
    let result = check_expr(&program.result, &scope, &mut errors);
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(TypedProgram {
        bindings,
        result,
        externals: externals.clone(),
        symbol_count: next_id as usize,
    })
}

fn check_bindings(
    bindings: &[LetStatement],
    scope: &mut HashMap<String, SymbolId>,
    next_id: &mut u32,
    errors: &mut Vec<TypeDiagnostic>,
    external: &HashMap<String, SymbolId>,
) -> Vec<TypedBinding> {
    let mut result = Vec::new();
    for binding in bindings {
        if external.contains_key(&binding.name) {
            errors.push(TypeDiagnostic {
                code: "reserved_binding",
                span: binding.name_span,
            });
        }
        if scope.contains_key(&binding.name) {
            errors.push(TypeDiagnostic {
                code: "duplicate_binding",
                span: binding.name_span,
            });
        }
        let value = check_expr(&binding.value, scope, errors);
        let id = SymbolId(*next_id);
        *next_id += 1;
        scope.insert(binding.name.clone(), id);
        result.push(TypedBinding {
            id,
            name: binding.name.clone(),
            value,
            span: binding.name_span,
        });
    }
    result
}

fn check_block(
    block: &Block,
    scope: &HashMap<String, SymbolId>,
    next_id: &mut u32,
    errors: &mut Vec<TypeDiagnostic>,
    external: &HashMap<String, SymbolId>,
) -> (Vec<TypedBinding>, TypedExpr) {
    let mut local_scope = scope.clone();
    let bindings = check_bindings(&block.bindings, &mut local_scope, next_id, errors, external);
    let result = check_expr(&block.result, &local_scope, errors);
    (bindings, result)
}

fn check_expr(
    expr: &Expr,
    scope: &HashMap<String, SymbolId>,
    errors: &mut Vec<TypeDiagnostic>,
) -> TypedExpr {
    let scalar = |kind| TypedExpr {
        kind,
        ty: ValueType::Scalar,
        span: expr.span,
    };
    match &expr.kind {
        ExprKind::Number(value) => scalar(TypedExprKind::Constant(*value)),
        ExprKind::Bool(value) => TypedExpr {
            kind: TypedExprKind::Bool(*value),
            ty: ValueType::Bool,
            span: expr.span,
        },
        ExprKind::Identifier(name) => match name.as_str() {
            "pi" => scalar(TypedExprKind::Constant(std::f32::consts::PI)),
            "e" => scalar(TypedExprKind::Constant(std::f32::consts::E)),
            _ => scope
                .get(name)
                .copied()
                .map(|id| scalar(TypedExprKind::Symbol(id)))
                .unwrap_or_else(|| {
                    errors.push(TypeDiagnostic {
                        code: "unknown_symbol",
                        span: expr.span,
                    });
                    scalar(TypedExprKind::Constant(0.0))
                }),
        },
        ExprKind::Unary { op, operand } => {
            let operand = check_expr(operand, scope, errors);
            match op {
                UnaryOp::Neg if operand.ty == ValueType::Scalar => scalar(TypedExprKind::Unary {
                    op: op.clone(),
                    operand: Box::new(operand),
                }),
                UnaryOp::Not if operand.ty == ValueType::Bool => TypedExpr {
                    kind: TypedExprKind::Unary {
                        op: op.clone(),
                        operand: Box::new(operand),
                    },
                    ty: ValueType::Bool,
                    span: expr.span,
                },
                _ => {
                    errors.push(TypeDiagnostic {
                        code: "expected_scalar",
                        span: expr.span,
                    });
                    scalar(TypedExprKind::Constant(0.0))
                }
            }
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let lhs = check_expr(lhs, scope, errors);
            let rhs = check_expr(rhs, scope, errors);
            let boolean = matches!(
                op,
                BinaryOp::And
                    | BinaryOp::Or
                    | BinaryOp::Equal
                    | BinaryOp::NotEqual
                    | BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual
            );
            if matches!(op, BinaryOp::And | BinaryOp::Or) {
                if lhs.ty != ValueType::Bool || rhs.ty != ValueType::Bool {
                    errors.push(TypeDiagnostic {
                        code: "expected_bool",
                        span: expr.span,
                    });
                }
            } else if lhs.ty != ValueType::Scalar || rhs.ty != ValueType::Scalar {
                errors.push(TypeDiagnostic {
                    code: "expected_scalar",
                    span: expr.span,
                });
            }
            TypedExpr {
                kind: TypedExprKind::Binary {
                    op: op.clone(),
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                ty: if boolean {
                    ValueType::Bool
                } else {
                    ValueType::Scalar
                },
                span: expr.span,
            }
        }
        ExprKind::Call { name, arguments } => {
            let arguments = arguments
                .iter()
                .map(|argument| check_expr(argument, scope, errors))
                .collect::<Vec<_>>();
            let expected = match name.as_str() {
                "sqrt" | "abs" | "exp" | "log" | "sin" | "cos" | "tanh" | "floor" | "ceil"
                | "round" | "sign" => Some(1),
                "min" | "max" | "step" => Some(2),
                "clamp" | "smoothstep" | "mix" | "gauss" => Some(3),
                _ => None,
            };
            if expected.is_none() {
                errors.push(TypeDiagnostic {
                    code: "unknown_function",
                    span: expr.span,
                });
            } else if expected != Some(arguments.len()) {
                errors.push(TypeDiagnostic {
                    code: "wrong_arity",
                    span: expr.span,
                });
            }
            if arguments
                .iter()
                .any(|argument| argument.ty != ValueType::Scalar)
            {
                errors.push(TypeDiagnostic {
                    code: "expected_scalar",
                    span: expr.span,
                });
            }
            scalar(TypedExprKind::Call {
                name: name.clone(),
                arguments,
            })
        }
        ExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            let condition = check_expr(condition, scope, errors);
            if condition.ty != ValueType::Bool {
                errors.push(TypeDiagnostic {
                    code: "expected_bool",
                    span: condition.span,
                });
            }
            let mut then_next = scope
                .values()
                .map(|id| id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let (then_bindings, then_result) =
                check_block(then_branch, scope, &mut then_next, errors, scope);
            let mut else_next = then_next;
            let (else_bindings, else_result) =
                check_block(else_branch, scope, &mut else_next, errors, scope);
            if then_result.ty != else_result.ty {
                errors.push(TypeDiagnostic {
                    code: "branch_type_mismatch",
                    span: expr.span,
                });
            }
            TypedExpr {
                kind: TypedExprKind::If {
                    condition: Box::new(condition),
                    then_branch: then_bindings,
                    then_result: Box::new(then_result.clone()),
                    else_branch: else_bindings,
                    else_result: Box::new(else_result),
                },
                ty: then_result.ty,
                span: expr.span,
            }
        }
    }
}

pub fn lint(_program: &TypedProgram) -> Vec<TypeDiagnostic> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn symbols(kernels: &[&str], parameters: &[&str]) -> ExternalSymbols {
        ExternalSymbols::new(kernels, parameters)
    }
    #[test]
    fn resolves_kernel_self_parameter_and_local_symbols() {
        let typed = compile(
            "let d = inner - mu; if self > 0.0 { d } else { -d }",
            &symbols(&["inner"], &["mu"]),
        )
        .unwrap();
        assert_eq!(typed.result.ty, ValueType::Scalar);
        assert_eq!(typed.externals.kernel_inputs.len(), 1);
    }
    #[test]
    fn forbids_shadowing_external_or_local_names() {
        assert!(
            compile("let inner = 1.0; inner", &symbols(&["inner"], &[]))
                .unwrap_err()
                .iter()
                .any(|error| error.code == "reserved_binding")
        );
        assert!(
            compile("let x = 1.0; let x = 2.0; x", &symbols(&[], &[]))
                .unwrap_err()
                .iter()
                .any(|error| error.code == "duplicate_binding")
        );
    }
    #[test]
    fn if_condition_is_boolean_and_branches_match() {
        assert!(
            compile("if 1.0 { 0.0 } else { 1.0 }", &symbols(&[], &[]))
                .unwrap_err()
                .iter()
                .any(|error| error.code == "expected_bool")
        );
        assert!(
            compile("if true { 0.0 } else { false }", &symbols(&[], &[]))
                .unwrap_err()
                .iter()
                .any(|error| error.code == "branch_type_mismatch")
        );
    }
}
