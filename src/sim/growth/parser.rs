use super::ast::*;
use super::lexer::{Span, Token, TokenKind, lex};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: &'static str,
    pub span: Span,
}

pub fn parse_program(source: &str) -> Result<Program, Vec<ParseError>> {
    let tokens = lex(source).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| ParseError {
                code: error.code,
                span: error.span,
            })
            .collect::<Vec<_>>()
    })?;
    Parser {
        tokens,
        index: 0,
        errors: Vec::new(),
    }
    .parse()
}

struct Parser {
    tokens: Vec<Token>,
    index: usize,
    errors: Vec<ParseError>,
}

impl Parser {
    fn parse(mut self) -> Result<Program, Vec<ParseError>> {
        let bindings = self.parse_bindings();
        let result = self.parse_expression(0);
        if self.check(&TokenKind::Semicolon) {
            self.errors.push(ParseError {
                code: "missing_result_expression",
                span: self.peek().span,
            });
            self.advance();
        }
        if !self.check(&TokenKind::Eof) {
            self.errors.push(ParseError {
                code: "unexpected_token",
                span: self.peek().span,
            });
        }
        if self.errors.is_empty() {
            Ok(Program { bindings, result })
        } else {
            Err(self.errors)
        }
    }

    fn parse_bindings(&mut self) -> Vec<LetStatement> {
        let mut bindings = Vec::new();
        while self.check(&TokenKind::Let) {
            let let_span = self.advance().span;
            let (name, name_span) = match self.advance().kind.clone() {
                TokenKind::Identifier(name) => (name, self.previous().span),
                _ => {
                    self.errors.push(ParseError {
                        code: "expected_identifier",
                        span: self.previous().span,
                    });
                    ("_error".into(), let_span)
                }
            };
            if !self.matches(&TokenKind::Equal) {
                self.errors.push(ParseError {
                    code: "expected_equal",
                    span: self.peek().span,
                });
            }
            let value = self.parse_expression(0);
            if !self.matches(&TokenKind::Semicolon) {
                self.errors.push(ParseError {
                    code: "expected_semicolon",
                    span: self.peek().span,
                });
            }
            bindings.push(LetStatement {
                name,
                name_span,
                value,
            });
        }
        bindings
    }

    fn parse_block(&mut self) -> Block {
        let bindings = self.parse_bindings();
        let result = self.parse_expression(0);
        if self.matches(&TokenKind::Semicolon) {
            self.errors.push(ParseError {
                code: "missing_result_expression",
                span: self.previous().span,
            });
        }
        if !self.matches(&TokenKind::RightBrace) {
            self.errors.push(ParseError {
                code: "expected_right_brace",
                span: self.peek().span,
            });
        }
        Block { bindings, result }
    }

    fn parse_expression(&mut self, minimum: u8) -> Expr {
        let mut lhs = self.parse_unary();
        while let Some((op, precedence)) = self.binary_operator() {
            if precedence < minimum {
                break;
            }
            self.advance();
            let rhs = self.parse_expression(precedence + u8::from(!matches!(op, BinaryOp::Power)));
            let span = Span::new(lhs.span.start, rhs.span.end);
            lhs = Expr {
                span,
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
            };
        }
        lhs
    }

    fn parse_unary(&mut self) -> Expr {
        let token = self.peek().clone();
        let op = match token.kind {
            TokenKind::Minus => Some(UnaryOp::Neg),
            TokenKind::Bang => Some(UnaryOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let operand = self.parse_unary();
            return Expr {
                span: Span::new(token.span.start, operand.span.end),
                kind: ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
            };
        }
        self.parse_primary()
    }

    fn parse_primary(&mut self) -> Expr {
        let token = self.advance();
        match token.kind {
            TokenKind::Number(value) => Expr {
                kind: ExprKind::Number(value),
                span: token.span,
            },
            TokenKind::True => Expr {
                kind: ExprKind::Bool(true),
                span: token.span,
            },
            TokenKind::False => Expr {
                kind: ExprKind::Bool(false),
                span: token.span,
            },
            TokenKind::Identifier(name) => {
                if self.matches(&TokenKind::LeftParen) {
                    let mut arguments = Vec::new();
                    if !self.check(&TokenKind::RightParen) {
                        loop {
                            arguments.push(self.parse_expression(0));
                            if !self.matches(&TokenKind::Comma) {
                                break;
                            }
                        }
                    }
                    let end = if self.matches(&TokenKind::RightParen) {
                        self.previous().span.end
                    } else {
                        self.errors.push(ParseError {
                            code: "expected_right_paren",
                            span: self.peek().span,
                        });
                        self.peek().span.end
                    };
                    Expr {
                        kind: ExprKind::Call { name, arguments },
                        span: Span::new(token.span.start, end),
                    }
                } else {
                    Expr {
                        kind: ExprKind::Identifier(name),
                        span: token.span,
                    }
                }
            }
            TokenKind::If => {
                let condition = self.parse_expression(0);
                if !self.matches(&TokenKind::LeftBrace) {
                    self.errors.push(ParseError {
                        code: "expected_left_brace",
                        span: self.peek().span,
                    });
                }
                let then_branch = self.parse_block();
                if !self.matches(&TokenKind::Else) {
                    self.errors.push(ParseError {
                        code: "expected_else",
                        span: self.peek().span,
                    });
                }
                if !self.matches(&TokenKind::LeftBrace) {
                    self.errors.push(ParseError {
                        code: "expected_left_brace",
                        span: self.peek().span,
                    });
                }
                let else_branch = self.parse_block();
                Expr {
                    span: Span::new(token.span.start, else_branch.result.span.end),
                    kind: ExprKind::If {
                        condition: Box::new(condition),
                        then_branch: Box::new(then_branch),
                        else_branch: Box::new(else_branch),
                    },
                }
            }
            TokenKind::LeftParen => {
                let expression = self.parse_expression(0);
                if !self.matches(&TokenKind::RightParen) {
                    self.errors.push(ParseError {
                        code: "expected_right_paren",
                        span: self.peek().span,
                    });
                }
                expression
            }
            _ => {
                self.errors.push(ParseError {
                    code: "expected_expression",
                    span: token.span,
                });
                Expr {
                    kind: ExprKind::Number(0.0),
                    span: token.span,
                }
            }
        }
    }

    fn binary_operator(&self) -> Option<(BinaryOp, u8)> {
        Some(match self.peek().kind {
            TokenKind::OrOr => (BinaryOp::Or, 1),
            TokenKind::AndAnd => (BinaryOp::And, 2),
            TokenKind::EqualEqual => (BinaryOp::Equal, 3),
            TokenKind::BangEqual => (BinaryOp::NotEqual, 3),
            TokenKind::Less => (BinaryOp::Less, 4),
            TokenKind::LessEqual => (BinaryOp::LessEqual, 4),
            TokenKind::Greater => (BinaryOp::Greater, 4),
            TokenKind::GreaterEqual => (BinaryOp::GreaterEqual, 4),
            TokenKind::Plus => (BinaryOp::Add, 5),
            TokenKind::Minus => (BinaryOp::Subtract, 5),
            TokenKind::Star => (BinaryOp::Multiply, 6),
            TokenKind::Slash => (BinaryOp::Divide, 6),
            TokenKind::Caret => (BinaryOp::Power, 7),
            _ => return None,
        })
    }
    fn check(&self, kind: &TokenKind) -> bool {
        std::mem::discriminant(&self.peek().kind) == std::mem::discriminant(kind)
    }
    fn matches(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn advance(&mut self) -> Token {
        let token = self.peek().clone();
        self.index = self
            .index
            .saturating_add(1)
            .min(self.tokens.len().saturating_sub(1));
        token
    }
    fn peek(&self) -> &Token {
        &self.tokens[self.index]
    }
    fn previous(&self) -> &Token {
        &self.tokens[self.index.saturating_sub(1)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_let_bindings_and_if_as_an_expression() {
        let program =
            parse_program("let a = inner * 2.0;\nif a > self { a } else { self }").unwrap();
        assert_eq!(program.bindings.len(), 1);
        assert!(matches!(program.result.kind, ExprKind::If { .. }));
    }
    #[test]
    fn multiplication_binds_tighter_than_addition() {
        let program = parse_program("1.0 + 2.0 * 3.0").unwrap();
        assert!(matches!(
            program.result.kind,
            ExprKind::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
    }
    #[test]
    fn final_expression_must_not_end_with_semicolon() {
        let errors = parse_program("let x = 1.0; x;").unwrap_err();
        assert!(
            errors
                .iter()
                .any(|error| error.code == "missing_result_expression")
        );
    }
}
