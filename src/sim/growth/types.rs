use super::ast::{BinaryOp, UnaryOp};
use super::lexer::Span;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SymbolId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueType {
    Scalar,
    Bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExternalSymbols {
    pub kernel_inputs: Vec<String>,
    pub parameters: Vec<String>,
}

impl ExternalSymbols {
    pub fn new(kernel_inputs: &[&str], parameters: &[&str]) -> Self {
        Self {
            kernel_inputs: kernel_inputs
                .iter()
                .map(|name| (*name).to_string())
                .collect(),
            parameters: parameters.iter().map(|name| (*name).to_string()).collect(),
        }
    }
    pub fn ordered(&self) -> Vec<String> {
        let mut names = self.kernel_inputs.clone();
        names.push("self".to_string());
        let mut parameters = self.parameters.clone();
        parameters.sort();
        names.extend(parameters);
        names
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedProgram {
    pub bindings: Vec<TypedBinding>,
    pub result: TypedExpr,
    pub externals: ExternalSymbols,
    pub symbol_count: usize,
}

impl TypedProgram {
    /// External symbols the program actually mentions, in the order the
    /// signature declares them.
    ///
    /// A signature says what a program *may* read; this says what it *does*.
    /// The distinction matters wherever the answer drives a choice on the
    /// user's behalf, such as which inputs a plot should vary: defaulting to
    /// every declared input would plot axes the program ignores.
    pub fn referenced_externals(&self) -> Vec<String> {
        let names = self.externals.ordered();
        let mut seen = vec![false; names.len()];
        for binding in &self.bindings {
            mark(&binding.value, &mut seen);
        }
        mark(&self.result, &mut seen);
        names
            .into_iter()
            .zip(seen)
            .filter_map(|(name, used)| used.then_some(name))
            .collect()
    }

    /// Kernel inputs the program actually mentions, in signature order.
    pub fn referenced_kernel_inputs(&self) -> Vec<String> {
        let referenced = self.referenced_externals();
        self.externals
            .kernel_inputs
            .iter()
            .filter(|name| referenced.contains(name))
            .cloned()
            .collect()
    }
}

/// External symbols are numbered before every local binding, so an id below
/// the external count identifies one of them.
fn mark(expr: &TypedExpr, seen: &mut [bool]) {
    match &expr.kind {
        TypedExprKind::Constant(_) | TypedExprKind::Bool(_) => {}
        TypedExprKind::Symbol(id) => {
            if let Some(slot) = seen.get_mut(id.0 as usize) {
                *slot = true;
            }
        }
        TypedExprKind::Unary { operand, .. } => mark(operand, seen),
        TypedExprKind::Binary { lhs, rhs, .. } => {
            mark(lhs, seen);
            mark(rhs, seen);
        }
        TypedExprKind::Call { arguments, .. } => {
            for argument in arguments {
                mark(argument, seen);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            then_result,
            else_branch,
            else_result,
        } => {
            mark(condition, seen);
            for binding in then_branch {
                mark(&binding.value, seen);
            }
            mark(then_result, seen);
            for binding in else_branch {
                mark(&binding.value, seen);
            }
            mark(else_result, seen);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedBinding {
    pub id: SymbolId,
    pub name: String,
    pub value: TypedExpr,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TypedExpr {
    pub kind: TypedExprKind,
    pub ty: ValueType,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TypedExprKind {
    Constant(f32),
    Bool(bool),
    Symbol(SymbolId),
    Unary {
        op: UnaryOp,
        operand: Box<TypedExpr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<TypedExpr>,
        rhs: Box<TypedExpr>,
    },
    Call {
        name: String,
        arguments: Vec<TypedExpr>,
    },
    If {
        condition: Box<TypedExpr>,
        then_branch: Vec<TypedBinding>,
        then_result: Box<TypedExpr>,
        else_branch: Vec<TypedBinding>,
        else_result: Box<TypedExpr>,
    },
}
