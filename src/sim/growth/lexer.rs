#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Number(f32),
    Let,
    If,
    Else,
    True,
    False,
    Plus,
    Minus,
    Star,
    Slash,
    Caret,
    Bang,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Semicolon,
    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, range: std::ops::Range<usize>) -> Self {
        Self {
            kind,
            span: Span::new(range.start, range.end),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LexError {
    pub span: Span,
    pub code: &'static str,
}

pub fn lex(source: &str) -> Result<Vec<Token>, Vec<LexError>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut errors = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if byte == b'/' && bytes.get(index + 1) == Some(&b'/') {
            index += 2;
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        let start = index;
        if byte.is_ascii_alphabetic() || byte == b'_' {
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            let name = &source[start..index];
            let kind = match name {
                "let" => TokenKind::Let,
                "if" => TokenKind::If,
                "else" => TokenKind::Else,
                "true" => TokenKind::True,
                "false" => TokenKind::False,
                _ => TokenKind::Identifier(name.to_string()),
            };
            tokens.push(Token::new(kind, start..index));
            continue;
        }
        if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
        {
            index += 1;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            if bytes
                .get(index)
                .is_some_and(|byte| *byte == b'e' || *byte == b'E')
            {
                index += 1;
                if bytes
                    .get(index)
                    .is_some_and(|byte| *byte == b'+' || *byte == b'-')
                {
                    index += 1;
                }
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
            }
            let literal = &source[start..index];
            match literal.parse::<f32>() {
                Ok(value) if value.is_finite() => {
                    tokens.push(Token::new(TokenKind::Number(value), start..index))
                }
                _ => errors.push(LexError {
                    span: Span::new(start, index),
                    code: "invalid_number",
                }),
            }
            continue;
        }
        let (kind, width) = match (byte, bytes.get(index + 1).copied()) {
            (b'+', _) => (Some(TokenKind::Plus), 1),
            (b'-', _) => (Some(TokenKind::Minus), 1),
            (b'*', _) => (Some(TokenKind::Star), 1),
            (b'/', _) => (Some(TokenKind::Slash), 1),
            (b'^', _) => (Some(TokenKind::Caret), 1),
            (b'(', _) => (Some(TokenKind::LeftParen), 1),
            (b')', _) => (Some(TokenKind::RightParen), 1),
            (b'{', _) => (Some(TokenKind::LeftBrace), 1),
            (b'}', _) => (Some(TokenKind::RightBrace), 1),
            (b',', _) => (Some(TokenKind::Comma), 1),
            (b';', _) => (Some(TokenKind::Semicolon), 1),
            (b'!', Some(b'=')) => (Some(TokenKind::BangEqual), 2),
            (b'=', Some(b'=')) => (Some(TokenKind::EqualEqual), 2),
            (b'<', Some(b'=')) => (Some(TokenKind::LessEqual), 2),
            (b'>', Some(b'=')) => (Some(TokenKind::GreaterEqual), 2),
            (b'&', Some(b'&')) => (Some(TokenKind::AndAnd), 2),
            (b'|', Some(b'|')) => (Some(TokenKind::OrOr), 2),
            (b'!', _) => (Some(TokenKind::Bang), 1),
            (b'=', _) => (Some(TokenKind::Equal), 1),
            (b'<', _) => (Some(TokenKind::Less), 1),
            (b'>', _) => (Some(TokenKind::Greater), 1),
            _ => (None, 1),
        };
        if let Some(kind) = kind {
            index += width;
            tokens.push(Token::new(kind, start..index));
        } else {
            errors.push(LexError {
                span: Span::new(start, start + 1),
                code: "invalid_character",
            });
            index += 1;
        }
    }
    tokens.push(Token::new(TokenKind::Eof, source.len()..source.len()));
    if errors.is_empty() {
        Ok(tokens)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lexes_let_if_comments_and_boolean_operators() {
        let source = "let x = inner; // signal\nif x >= 0.5 && self != 0.0 { x } else { -x }";
        let tokens = lex(source).unwrap();
        assert_eq!(tokens[0], Token::new(TokenKind::Let, 0..3));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::AndAnd));
        assert!(tokens.iter().any(|token| token.kind == TokenKind::Else));
        assert!(!tokens.iter().any(|token| matches!(
            token.kind,
            TokenKind::Identifier(ref name) if name == "signal"
        )));
    }
    #[test]
    fn reports_every_invalid_character_in_one_pass() {
        let errors = lex("inner @ outer $ self").unwrap_err();
        assert_eq!(
            errors
                .iter()
                .map(|error| error.span.start)
                .collect::<Vec<_>>(),
            vec![6, 14]
        );
    }
}
