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
