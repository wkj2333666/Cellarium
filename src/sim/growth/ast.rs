use super::lexer::Span;

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub bindings: Vec<LetStatement>,
    pub result: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub bindings: Vec<LetStatement>,
    pub result: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LetStatement {
    pub name: String,
    pub name_span: Span,
    pub value: Expr,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Number(f32),
    Bool(bool),
    Identifier(String),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        name: String,
        arguments: Vec<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_branch: Box<Block>,
        else_branch: Box<Block>,
    },
}
