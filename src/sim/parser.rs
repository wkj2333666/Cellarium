use std::collections::BTreeSet;

use super::expression::{BinaryOp, ExpressionVariable, Function, KernelExpression, UnaryOp};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("unexpected token at byte {position}: {message}")]
    UnexpectedToken { position: usize, message: String },
    #[error("invalid number at byte {position}: {literal}")]
    InvalidNumber { position: usize, literal: String },
    #[error("unknown function `{name}` at byte {position}")]
    UnknownFunction { position: usize, name: String },
    #[error("wrong arity for `{name}` at byte {position}: expected {expected}, got {found}")]
    WrongFunctionArity {
        position: usize,
        name: String,
        expected: usize,
        found: usize,
    },
    #[error("unknown parameter `{name}` at byte {position}")]
    UnknownParameter { position: usize, name: String },
    #[error("trailing input at byte {position}: {token}")]
    TrailingInput { position: usize, token: String },
    #[error("unexpected end of input at byte {position}")]
    UnexpectedEnd { position: usize },
}

pub fn parse_expression(source: &str) -> Result<KernelExpression, ParseError> {
    Parser::new(source).parse()
}

pub fn parse_and_validate(
    source: &str,
    parameters: &BTreeSet<String>,
) -> Result<KernelExpression, ParseError> {
    let (expression, parameter_positions) = Parser::new(source).parse_with_positions()?;
    if let Err(error) = validate_symbols(&expression, parameters) {
        if let ParseError::UnknownParameter { name, .. } = &error
            && let Some((_, position)) = parameter_positions
                .iter()
                .find(|(candidate, _)| candidate == name)
        {
            return Err(ParseError::UnknownParameter {
                position: *position,
                name: name.clone(),
            });
        }
        return Err(error);
    }
    Ok(expression)
}

pub fn validate_symbols(
    expression: &KernelExpression,
    parameters: &BTreeSet<String>,
) -> Result<(), ParseError> {
    match expression {
        KernelExpression::Constant(_) | KernelExpression::Variable(_) => Ok(()),
        KernelExpression::Parameter(name) => {
            if parameters.contains(name) {
                Ok(())
            } else {
                Err(ParseError::UnknownParameter {
                    position: 0,
                    name: name.clone(),
                })
            }
        }
        KernelExpression::Binary { lhs, rhs, .. } => {
            validate_symbols(lhs, parameters)?;
            validate_symbols(rhs, parameters)
        }
        KernelExpression::Unary { operand, .. } => validate_symbols(operand, parameters),
        KernelExpression::Call { arguments, .. } => arguments
            .iter()
            .try_for_each(|argument| validate_symbols(argument, parameters)),
    }
}

pub fn fold_constants(expression: KernelExpression) -> KernelExpression {
    match expression {
        KernelExpression::Constant(_)
        | KernelExpression::Parameter(_)
        | KernelExpression::Variable(_) => expression,
        KernelExpression::Binary { op, lhs, rhs } => {
            let lhs = fold_constants(*lhs);
            let rhs = fold_constants(*rhs);
            if let (KernelExpression::Constant(lhs), KernelExpression::Constant(rhs)) = (&lhs, &rhs)
                && let Some(value) = fold_binary(op, *lhs, *rhs)
            {
                return KernelExpression::Constant(value);
            }
            KernelExpression::Binary {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }
        }
        KernelExpression::Unary { op, operand } => {
            let operand = fold_constants(*operand);
            if let KernelExpression::Constant(value) = operand {
                if let Some(value) = fold_unary(op, value) {
                    return KernelExpression::Constant(value);
                }
                return KernelExpression::Unary {
                    op,
                    operand: Box::new(KernelExpression::Constant(value)),
                };
            }
            KernelExpression::Unary {
                op,
                operand: Box::new(operand),
            }
        }
        KernelExpression::Call {
            function,
            arguments,
        } => {
            let arguments = arguments
                .into_iter()
                .map(fold_constants)
                .collect::<Vec<_>>();
            if arguments
                .iter()
                .all(|argument| matches!(argument, KernelExpression::Constant(_)))
            {
                let values = arguments
                    .iter()
                    .map(|argument| match argument {
                        KernelExpression::Constant(value) => *value,
                        _ => unreachable!(),
                    })
                    .collect::<Vec<_>>();
                if let Some(value) = fold_call(function, &values) {
                    return KernelExpression::Constant(value);
                }
            }
            KernelExpression::Call {
                function,
                arguments,
            }
        }
    }
}

pub fn simplify(expression: KernelExpression) -> KernelExpression {
    simplify_node(fold_constants(expression))
}

pub fn format_expression(expression: &KernelExpression) -> String {
    format_node(expression)
}

struct Parser<'a> {
    source: &'a str,
    position: usize,
    parameter_positions: Vec<(String, usize)>,
}

impl<'a> Parser<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            position: 0,
            parameter_positions: Vec::new(),
        }
    }

    fn parse(self) -> Result<KernelExpression, ParseError> {
        self.parse_with_positions()
            .map(|(expression, _)| expression)
    }

    fn parse_with_positions(
        mut self,
    ) -> Result<(KernelExpression, Vec<(String, usize)>), ParseError> {
        self.skip_whitespace();
        if self.is_at_end() {
            return Err(ParseError::UnexpectedEnd {
                position: self.position,
            });
        }
        let expression = self.parse_additive()?;
        self.skip_whitespace();
        if !self.is_at_end() {
            return Err(ParseError::TrailingInput {
                position: self.position,
                token: self.next_char().unwrap().to_string(),
            });
        }
        Ok((expression, self.parameter_positions))
    }

    fn parse_additive(&mut self) -> Result<KernelExpression, ParseError> {
        let mut expression = self.parse_multiplicative()?;
        loop {
            self.skip_whitespace();
            let op = match self.next_char() {
                Some('+') => BinaryOp::Add,
                Some('-') => BinaryOp::Subtract,
                _ => break,
            };
            self.position += 1;
            let rhs = self.parse_multiplicative()?;
            expression = binary(op, expression, rhs);
        }
        Ok(expression)
    }

    fn parse_multiplicative(&mut self) -> Result<KernelExpression, ParseError> {
        let mut expression = self.parse_unary()?;
        loop {
            self.skip_whitespace();
            let op = match self.next_char() {
                Some('*') => BinaryOp::Multiply,
                Some('/') => BinaryOp::Divide,
                _ => break,
            };
            self.position += 1;
            let rhs = self.parse_unary()?;
            expression = binary(op, expression, rhs);
        }
        Ok(expression)
    }

    fn parse_unary(&mut self) -> Result<KernelExpression, ParseError> {
        self.skip_whitespace();
        match self.next_char() {
            Some('+') => {
                self.position += 1;
                self.parse_unary()
            }
            Some('-') => {
                self.position += 1;
                Ok(unary(UnaryOp::Neg, self.parse_unary()?))
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> Result<KernelExpression, ParseError> {
        let lhs = self.parse_primary()?;
        self.skip_whitespace();
        if self.next_char() == Some('^') {
            self.position += 1;
            let rhs = self.parse_unary()?;
            Ok(binary(BinaryOp::Power, lhs, rhs))
        } else {
            Ok(lhs)
        }
    }

    fn parse_primary(&mut self) -> Result<KernelExpression, ParseError> {
        self.skip_whitespace();
        match self.next_char() {
            Some(character) if character.is_ascii_digit() || character == '.' => {
                self.parse_number()
            }
            Some(character) if is_identifier_start(character) => self.parse_identifier_or_call(),
            Some('(') => {
                self.position += 1;
                let expression = self.parse_additive()?;
                self.skip_whitespace();
                match self.next_char() {
                    Some(')') => {
                        self.position += 1;
                        Ok(expression)
                    }
                    Some(character) => Err(ParseError::UnexpectedToken {
                        position: self.position,
                        message: format!("expected `)`, found `{character}`"),
                    }),
                    None => Err(ParseError::UnexpectedEnd {
                        position: self.position,
                    }),
                }
            }
            Some(character) => Err(ParseError::UnexpectedToken {
                position: self.position,
                message: format!("unexpected `{character}`"),
            }),
            None => Err(ParseError::UnexpectedEnd {
                position: self.position,
            }),
        }
    }

    fn parse_number(&mut self) -> Result<KernelExpression, ParseError> {
        let start = self.position;
        let mut has_digits = false;
        while self
            .next_char()
            .is_some_and(|character| character.is_ascii_digit())
        {
            has_digits = true;
            self.position += 1;
        }
        if self.next_char() == Some('.') {
            self.position += 1;
            while self
                .next_char()
                .is_some_and(|character| character.is_ascii_digit())
            {
                has_digits = true;
                self.position += 1;
            }
        }
        if !has_digits {
            return Err(ParseError::InvalidNumber {
                position: start,
                literal: self.source[start..self.position].to_string(),
            });
        }
        if self
            .next_char()
            .is_some_and(|character| matches!(character, 'e' | 'E'))
        {
            self.position += 1;
            if self
                .next_char()
                .is_some_and(|character| matches!(character, '+' | '-'))
            {
                self.position += 1;
            }
            let exponent_start = self.position;
            while self
                .next_char()
                .is_some_and(|character| character.is_ascii_digit())
            {
                self.position += 1;
            }
            if exponent_start == self.position {
                return Err(ParseError::InvalidNumber {
                    position: start,
                    literal: self.source[start..self.position].to_string(),
                });
            }
        }
        if self.next_char() == Some('.') {
            self.position += 1;
            return Err(ParseError::InvalidNumber {
                position: start,
                literal: self.source[start..self.position].to_string(),
            });
        }
        let literal = &self.source[start..self.position];
        match literal.parse::<f32>() {
            Ok(value) if value.is_finite() => Ok(KernelExpression::Constant(value)),
            _ => Err(ParseError::InvalidNumber {
                position: start,
                literal: literal.to_string(),
            }),
        }
    }

    fn parse_identifier_or_call(&mut self) -> Result<KernelExpression, ParseError> {
        let start = self.position;
        self.position += self.next_char().unwrap().len_utf8() - 1;
        while self.next_char().is_some_and(is_identifier_continue) {
            self.position += self.next_char().unwrap().len_utf8();
        }
        let name = &self.source[start..self.position];
        self.skip_whitespace();
        if self.next_char() != Some('(') {
            return Ok(match name {
                "x" => KernelExpression::Variable(ExpressionVariable::X),
                "y" => KernelExpression::Variable(ExpressionVariable::Y),
                "radius" => KernelExpression::Variable(ExpressionVariable::Radius),
                "distance" | "r" => KernelExpression::Variable(ExpressionVariable::Distance),
                _ => {
                    self.parameter_positions.push((name.to_string(), start));
                    KernelExpression::Parameter(name.to_string())
                }
            });
        }
        self.position += 1;
        let mut arguments = Vec::new();
        self.skip_whitespace();
        if self.next_char() != Some(')') {
            loop {
                arguments.push(self.parse_additive()?);
                self.skip_whitespace();
                match self.next_char() {
                    Some(',') => {
                        self.position += 1;
                        self.skip_whitespace();
                    }
                    Some(')') => break,
                    Some(character) => {
                        return Err(ParseError::UnexpectedToken {
                            position: self.position,
                            message: format!("expected `,` or `)`, found `{character}`"),
                        });
                    }
                    None => {
                        return Err(ParseError::UnexpectedEnd {
                            position: self.position,
                        });
                    }
                }
            }
        }
        if self.next_char() == Some(')') {
            self.position += 1;
        }
        let function = match name {
            "exp" => FunctionKind::Unary(UnaryOp::Exp),
            "sin" => FunctionKind::Unary(UnaryOp::Sin),
            "cos" => FunctionKind::Unary(UnaryOp::Cos),
            "sqrt" => FunctionKind::Unary(UnaryOp::Sqrt),
            "abs" => FunctionKind::Unary(UnaryOp::Abs),
            "min" => FunctionKind::Call(Function::Min, 2),
            "max" => FunctionKind::Call(Function::Max, 2),
            "clamp" => FunctionKind::Call(Function::Clamp, 3),
            _ => {
                return Err(ParseError::UnknownFunction {
                    position: start,
                    name: name.to_string(),
                });
            }
        };
        match function {
            FunctionKind::Unary(op) => {
                if arguments.len() != 1 {
                    return Err(ParseError::WrongFunctionArity {
                        position: start,
                        name: name.to_string(),
                        expected: 1,
                        found: arguments.len(),
                    });
                }
                Ok(unary(op, arguments.pop().unwrap()))
            }
            FunctionKind::Call(function, expected) => {
                if arguments.len() != expected {
                    return Err(ParseError::WrongFunctionArity {
                        position: start,
                        name: name.to_string(),
                        expected,
                        found: arguments.len(),
                    });
                }
                Ok(KernelExpression::Call {
                    function,
                    arguments,
                })
            }
        }
    }

    fn skip_whitespace(&mut self) {
        while self.next_char().is_some_and(char::is_whitespace) {
            self.position += self.next_char().unwrap().len_utf8();
        }
    }

    fn next_char(&self) -> Option<char> {
        self.source[self.position..].chars().next()
    }

    fn is_at_end(&self) -> bool {
        self.position >= self.source.len()
    }
}

enum FunctionKind {
    Unary(UnaryOp),
    Call(Function, usize),
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

fn binary(op: BinaryOp, lhs: KernelExpression, rhs: KernelExpression) -> KernelExpression {
    KernelExpression::Binary {
        op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn unary(op: UnaryOp, operand: KernelExpression) -> KernelExpression {
    KernelExpression::Unary {
        op,
        operand: Box::new(operand),
    }
}

fn fold_binary(op: BinaryOp, lhs: f32, rhs: f32) -> Option<f32> {
    let value = match op {
        BinaryOp::Add => lhs + rhs,
        BinaryOp::Subtract => lhs - rhs,
        BinaryOp::Multiply => lhs * rhs,
        BinaryOp::Divide if rhs != 0.0 => lhs / rhs,
        BinaryOp::Power => lhs.powf(rhs),
        BinaryOp::Divide => return None,
    };
    value.is_finite().then_some(value)
}

fn fold_unary(op: UnaryOp, value: f32) -> Option<f32> {
    let result = match op {
        UnaryOp::Neg => -value,
        UnaryOp::Sqrt if value >= 0.0 => value.sqrt(),
        UnaryOp::Abs => value.abs(),
        UnaryOp::Exp => value.exp(),
        UnaryOp::Sin => value.sin(),
        UnaryOp::Cos => value.cos(),
        UnaryOp::Sqrt => return None,
    };
    result.is_finite().then_some(result)
}

fn fold_call(function: Function, arguments: &[f32]) -> Option<f32> {
    let value = match function {
        Function::Min if arguments.len() == 2 => arguments[0].min(arguments[1]),
        Function::Max if arguments.len() == 2 => arguments[0].max(arguments[1]),
        Function::Clamp if arguments.len() == 3 && arguments[1] <= arguments[2] => {
            arguments[0].clamp(arguments[1], arguments[2])
        }
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn simplify_node(expression: KernelExpression) -> KernelExpression {
    match expression {
        KernelExpression::Binary { op, lhs, rhs } => {
            let lhs = simplify_node(*lhs);
            let rhs = simplify_node(*rhs);
            match (&op, &lhs, &rhs) {
                (BinaryOp::Add, _, KernelExpression::Constant(value))
                    if value.is_sign_positive() && *value == 0.0 =>
                {
                    lhs
                }
                (BinaryOp::Subtract, _, KernelExpression::Constant(value))
                    if value.is_sign_positive() && *value == 0.0 =>
                {
                    lhs
                }
                (BinaryOp::Multiply, _, KernelExpression::Constant(value)) if *value == 1.0 => lhs,
                (BinaryOp::Divide, _, KernelExpression::Constant(value)) if *value == 1.0 => lhs,
                (BinaryOp::Power, _, KernelExpression::Constant(value)) if *value == 1.0 => lhs,
                _ => binary(op, lhs, rhs),
            }
        }
        KernelExpression::Unary {
            op: UnaryOp::Neg,
            operand,
        } => match *operand {
            KernelExpression::Unary {
                op: UnaryOp::Neg,
                operand,
            } => simplify_node(*operand),
            operand => unary(UnaryOp::Neg, simplify_node(operand)),
        },
        KernelExpression::Unary { op, operand } => unary(op, simplify_node(*operand)),
        KernelExpression::Call {
            function,
            arguments,
        } => KernelExpression::Call {
            function,
            arguments: arguments.into_iter().map(simplify_node).collect(),
        },
        leaf => leaf,
    }
}

fn format_node(expression: &KernelExpression) -> String {
    match expression {
        KernelExpression::Constant(value) => value.to_string(),
        KernelExpression::Parameter(name) => name.clone(),
        KernelExpression::Variable(variable) => match variable {
            ExpressionVariable::X => "x".to_string(),
            ExpressionVariable::Y => "y".to_string(),
            ExpressionVariable::Radius => "radius".to_string(),
            ExpressionVariable::Distance => "distance".to_string(),
        },
        KernelExpression::Unary { op, operand } => match op {
            UnaryOp::Neg => {
                let needs_parentheses = matches!(operand.as_ref(), KernelExpression::Binary { .. });
                let operand = format_node(operand);
                if needs_parentheses {
                    format!("-({operand})")
                } else {
                    format!("-{operand}")
                }
            }
            UnaryOp::Sqrt => format!("sqrt({})", format_node(operand)),
            UnaryOp::Abs => format!("abs({})", format_node(operand)),
            UnaryOp::Exp => format!("exp({})", format_node(operand)),
            UnaryOp::Sin => format!("sin({})", format_node(operand)),
            UnaryOp::Cos => format!("cos({})", format_node(operand)),
        },
        KernelExpression::Binary { op, lhs, rhs } => {
            let symbol = match op {
                BinaryOp::Add => "+",
                BinaryOp::Subtract => "-",
                BinaryOp::Multiply => "*",
                BinaryOp::Divide => "/",
                BinaryOp::Power => "^",
            };
            let lhs = format_binary_child(lhs, *op, false);
            let rhs = format_binary_child(rhs, *op, true);
            format!("{lhs} {symbol} {rhs}")
        }
        KernelExpression::Call {
            function,
            arguments,
        } => {
            let name = match function {
                Function::Min => "min",
                Function::Max => "max",
                Function::Clamp => "clamp",
            };
            let arguments = arguments
                .iter()
                .map(format_node)
                .collect::<Vec<_>>()
                .join(", ");
            format!("{name}({arguments})")
        }
    }
}

fn precedence(expression: &KernelExpression) -> u8 {
    match expression {
        KernelExpression::Binary {
            op: BinaryOp::Add | BinaryOp::Subtract,
            ..
        } => 1,
        KernelExpression::Binary {
            op: BinaryOp::Multiply | BinaryOp::Divide,
            ..
        } => 2,
        KernelExpression::Binary {
            op: BinaryOp::Power,
            ..
        }
        | KernelExpression::Unary {
            op: UnaryOp::Neg, ..
        } => 3,
        _ => 4,
    }
}

fn format_binary_child(expression: &KernelExpression, parent: BinaryOp, rhs: bool) -> String {
    let formatted = format_node(expression);
    let child_precedence = precedence(expression);
    let parent_precedence = match parent {
        BinaryOp::Add | BinaryOp::Subtract => 1,
        BinaryOp::Multiply | BinaryOp::Divide => 2,
        BinaryOp::Power => 3,
    };
    let needs_parentheses = child_precedence < parent_precedence
        || (child_precedence == parent_precedence
            && if parent == BinaryOp::Power { !rhs } else { rhs });
    if needs_parentheses {
        format!("({formatted})")
    } else {
        formatted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::expression::{
        BinaryOp, ExpressionContext, ExpressionVariable, Function, UnaryOp, evaluate,
    };
    use std::collections::{BTreeMap, BTreeSet};

    fn parse(source: &str) -> KernelExpression {
        parse_expression(source).unwrap()
    }

    fn constant(value: f32) -> KernelExpression {
        KernelExpression::Constant(value)
    }

    fn binary(op: BinaryOp, lhs: KernelExpression, rhs: KernelExpression) -> KernelExpression {
        KernelExpression::Binary {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        }
    }

    fn unary(op: UnaryOp, operand: KernelExpression) -> KernelExpression {
        KernelExpression::Unary {
            op,
            operand: Box::new(operand),
        }
    }

    #[test]
    fn parses_precedence_unary_and_right_associative_power() {
        assert_eq!(parse("1.25e2"), constant(125.0));
        assert_eq!(
            parse("1 + 2 * 3 ^ 2"),
            binary(
                BinaryOp::Add,
                constant(1.0),
                binary(
                    BinaryOp::Multiply,
                    constant(2.0),
                    binary(BinaryOp::Power, constant(3.0), constant(2.0)),
                ),
            )
        );
        assert_eq!(
            parse("2 ^ 3 ^ 2"),
            binary(
                BinaryOp::Power,
                constant(2.0),
                binary(BinaryOp::Power, constant(3.0), constant(2.0)),
            )
        );
        assert_eq!(
            parse("-2^2"),
            unary(
                UnaryOp::Neg,
                binary(BinaryOp::Power, constant(2.0), constant(2.0))
            )
        );
        assert_eq!(
            parse("2^-2"),
            binary(
                BinaryOp::Power,
                constant(2.0),
                unary(UnaryOp::Neg, constant(2.0))
            )
        );
    }

    #[test]
    fn parses_all_variables_and_functions() {
        assert_eq!(
            parse("x"),
            KernelExpression::Variable(ExpressionVariable::X)
        );
        assert_eq!(
            parse("y"),
            KernelExpression::Variable(ExpressionVariable::Y)
        );
        assert_eq!(
            parse("radius"),
            KernelExpression::Variable(ExpressionVariable::Radius)
        );
        assert_eq!(
            parse("distance"),
            KernelExpression::Variable(ExpressionVariable::Distance)
        );
        assert_eq!(
            parse("r"),
            KernelExpression::Variable(ExpressionVariable::Distance)
        );
        assert_eq!(
            parse(
                "exp(1) + sin(2) + cos(3) + sqrt(4) + abs(-5) + min(1, 2) + max(3, 4) + clamp(5, 0, 1)"
            ),
            binary(
                BinaryOp::Add,
                binary(
                    BinaryOp::Add,
                    binary(
                        BinaryOp::Add,
                        binary(
                            BinaryOp::Add,
                            binary(
                                BinaryOp::Add,
                                binary(
                                    BinaryOp::Add,
                                    binary(
                                        BinaryOp::Add,
                                        unary(UnaryOp::Exp, constant(1.0)),
                                        unary(UnaryOp::Sin, constant(2.0))
                                    ),
                                    unary(UnaryOp::Cos, constant(3.0)),
                                ),
                                unary(UnaryOp::Sqrt, constant(4.0)),
                            ),
                            unary(UnaryOp::Abs, unary(UnaryOp::Neg, constant(5.0))),
                        ),
                        KernelExpression::Call {
                            function: Function::Min,
                            arguments: vec![constant(1.0), constant(2.0)]
                        },
                    ),
                    KernelExpression::Call {
                        function: Function::Max,
                        arguments: vec![constant(3.0), constant(4.0)]
                    },
                ),
                KernelExpression::Call {
                    function: Function::Clamp,
                    arguments: vec![constant(5.0), constant(0.0), constant(1.0)]
                },
            )
        );
    }

    #[test]
    fn validates_named_parameters_and_rejects_unknown_parameters() {
        let parameters = BTreeSet::from(["gain".to_string(), "offset".to_string()]);
        let expression = parse_and_validate("gain * x + offset", &parameters).unwrap();
        assert!(matches!(expression, KernelExpression::Binary { .. }));

        let error = parse_and_validate("gain * missing", &parameters).unwrap_err();
        assert!(matches!(
            error,
            ParseError::UnknownParameter { position: 7, .. }
        ));
    }

    #[test]
    fn reports_malformed_numbers_tokens_trailing_input_and_unknown_functions() {
        assert!(matches!(
            parse_expression("1e+"),
            Err(ParseError::InvalidNumber { .. })
        ));
        assert!(matches!(
            parse_expression("1..2"),
            Err(ParseError::InvalidNumber { .. })
        ));
        assert!(matches!(
            parse_expression("1 +"),
            Err(ParseError::UnexpectedEnd { .. })
        ));
        assert!(matches!(
            parse_expression("1 2"),
            Err(ParseError::TrailingInput { .. })
        ));
        assert!(matches!(
            parse_expression("noise(1)"),
            Err(ParseError::UnknownFunction { .. })
        ));
        assert!(matches!(
            parse_expression("min(1)"),
            Err(ParseError::WrongFunctionArity { .. })
        ));
        assert!(matches!(
            parse_expression("(1]"),
            Err(ParseError::UnexpectedToken { .. })
        ));
    }

    #[test]
    fn malformed_input_never_panics() {
        for source in ["", ")", "(", ".", "1e-", "min(,)", "clamp(1, 2,"] {
            let result = std::panic::catch_unwind(|| parse_expression(source));
            assert!(result.is_ok(), "parser panicked for {source:?}");
            assert!(result.unwrap().is_err(), "parser accepted {source:?}");
        }
    }

    #[test]
    fn folds_only_constant_subtrees() {
        assert_eq!(fold_constants(parse("1 + 2 * 3")), constant(7.0));
        assert_eq!(
            fold_constants(parse("x + 2 * 3")),
            binary(
                BinaryOp::Add,
                KernelExpression::Variable(ExpressionVariable::X),
                constant(6.0)
            )
        );
        assert_eq!(
            fold_constants(parse("1 / 0")),
            binary(BinaryOp::Divide, constant(1.0), constant(0.0))
        );
    }

    #[test]
    fn simplifies_safe_identities_without_erasing_variable_behavior() {
        assert_eq!(
            simplify(parse("(x + 0) * 1")),
            KernelExpression::Variable(ExpressionVariable::X)
        );
        assert_eq!(
            simplify(parse("x / 1")),
            KernelExpression::Variable(ExpressionVariable::X)
        );
        assert_eq!(
            simplify(parse("x ^ 1")),
            KernelExpression::Variable(ExpressionVariable::X)
        );
        assert_eq!(
            simplify(parse("--x")),
            KernelExpression::Variable(ExpressionVariable::X)
        );
        assert_eq!(
            simplify(parse("x * 0")),
            binary(
                BinaryOp::Multiply,
                KernelExpression::Variable(ExpressionVariable::X),
                constant(0.0)
            )
        );
    }

    #[test]
    fn pretty_prints_with_minimal_unambiguous_parentheses() {
        assert_eq!(
            format_expression(&parse("1 + 2 * (x - 3)")),
            "1 + 2 * (x - 3)"
        );
        assert_eq!(format_expression(&parse("2 ^ 3 ^ 2")), "2 ^ 3 ^ 2");
        let negative_base = parse("(-2) ^ 2");
        let formatted_negative_base = format_expression(&negative_base);
        assert_eq!(formatted_negative_base, "(-2) ^ 2");
        assert_eq!(parse(&formatted_negative_base), negative_base);
        assert_eq!(format_expression(&parse("-(x + 1)")), "-(x + 1)");
        assert_eq!(
            format_expression(&parse("clamp(abs(x), 0, 1)")),
            "clamp(abs(x), 0, 1)"
        );
    }

    #[test]
    fn parsed_expression_evaluates_independently_against_expression_context() {
        let parameters = BTreeSet::from(["gain".to_string()]);
        let expression = parse_and_validate(
            "clamp(abs(x) + sin(y), 0, max(radius, r)) * gain",
            &parameters,
        )
        .unwrap();
        let values = BTreeMap::from([(String::from("gain"), 0.5)]);
        let context = ExpressionContext {
            x: -0.25,
            y: 0.5,
            radius: 2.0,
            distance: 1.0,
            parameters: &values,
        };
        let actual = evaluate(&expression, &context).unwrap();
        let expected = ((-0.25_f32).abs() + 0.5_f32.sin()).clamp(0.0, 2.0) * 0.5;
        assert!((actual - expected).abs() < f32::EPSILON);
    }
}
