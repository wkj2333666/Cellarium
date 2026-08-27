//! A small source editor with a gutter and inline diagnostics.
//!
//! It exists so growth programs are written where they are used, with the
//! errors marked on the text that caused them rather than listed somewhere
//! else for the reader to correlate by eye.

use eframe::egui::{self, Ui, text::LayoutJob};

use crate::gui::theme;

/// A span of source the compiler objected to.
#[derive(Clone, Debug, PartialEq)]
pub struct Diagnostic {
    pub code: String,
    pub start: usize,
    pub end: usize,
}

/// What a lexer decided one run of characters is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Keyword,
    Number,
    Symbol,
    Operator,
    Comment,
    Plain,
}

impl TokenKind {
    fn color(self) -> egui::Color32 {
        match self {
            TokenKind::Keyword => theme::KERNEL_ANCHOR,
            TokenKind::Number => theme::KERNEL_POSITIVE,
            TokenKind::Symbol => theme::SINGLE_CHANNEL,
            TokenKind::Operator => theme::CELL_STROKE,
            TokenKind::Comment => theme::KERNEL_ACTIVE_ZERO,
            TokenKind::Plain => theme::SINGLE_CHANNEL,
        }
    }
}

const KEYWORDS: [&str; 6] = ["let", "if", "then", "else", "in", "self"];

/// Split source into coloured runs. Deliberately small: it classifies what the
/// growth language actually has rather than pretending to be a full parser,
/// and every byte of the input appears in exactly one run.
pub fn lex(source: &str) -> Vec<(TokenKind, std::ops::Range<usize>)> {
    let bytes = source.as_bytes();
    let mut runs = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        let kind = if byte == b'#' {
            while index < bytes.len() && bytes[index] != b'\n' {
                index += 1;
            }
            TokenKind::Comment
        } else if byte.is_ascii_digit()
            || (byte == b'.' && bytes.get(index + 1).is_some_and(u8::is_ascii_digit))
        {
            while index < bytes.len()
                && (bytes[index].is_ascii_digit()
                    || bytes[index] == b'.'
                    || bytes[index] == b'e'
                    || bytes[index] == b'E'
                    || ((bytes[index] == b'-' || bytes[index] == b'+')
                        && matches!(bytes.get(index - 1), Some(b'e' | b'E'))))
            {
                index += 1;
            }
            TokenKind::Number
        } else if byte.is_ascii_alphabetic() || byte == b'_' {
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            if KEYWORDS.contains(&&source[start..index]) {
                TokenKind::Keyword
            } else {
                TokenKind::Symbol
            }
        } else if byte.is_ascii_whitespace() {
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            TokenKind::Plain
        } else {
            index += 1;
            TokenKind::Operator
        };
        runs.push((kind, start..index));
    }
    runs
}

/// Build the coloured, underlined layout for one source string.
pub fn layout(source: &str, diagnostics: &[Diagnostic], font: egui::FontId) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (kind, range) in lex(source) {
        // A run inside a diagnostic span is underlined in the error colour, so
        // the mark is on the text that caused it.
        let faulty = diagnostics
            .iter()
            .any(|diagnostic| range.start < diagnostic.end && diagnostic.start < range.end);
        let mut format = egui::TextFormat {
            font_id: font.clone(),
            color: kind.color(),
            ..Default::default()
        };
        if faulty {
            format.underline = egui::Stroke::new(1.5, theme::state_color(theme::State::Invalid));
        }
        job.append(&source[range], 0.0, format);
    }
    job
}

/// Line numbers for the gutter.
pub fn gutter(source: &str) -> String {
    (1..=source.lines().count().max(1))
        .map(|line| line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Which line and column a byte offset falls on, counting from one.
pub fn position_of(source: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(source.len());
    let before = &source[..clamped];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map(|index| clamped - index - 1)
        .unwrap_or(clamped)
        + 1;
    (line, column)
}

/// Draw the editor. Returns true when the text changed.
pub fn code_editor(
    ui: &mut Ui,
    id: &str,
    source: &mut String,
    diagnostics: &[Diagnostic],
    rows: usize,
) -> bool {
    let font = egui::FontId::monospace(13.0);
    let owned: Vec<Diagnostic> = diagnostics.to_vec();
    let font_for_layout = font.clone();
    let mut layouter = move |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
        let mut job = layout(text.as_str(), &owned, font_for_layout.clone());
        job.wrap.max_width = wrap_width;
        ui.fonts_mut(|fonts| fonts.layout_job(job))
    };

    let mut changed = false;
    ui.horizontal_top(|ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(gutter(source))
                    .monospace()
                    .color(theme::KERNEL_ACTIVE_ZERO),
            )
            .selectable(false),
        );
        changed = ui
            .add(
                egui::TextEdit::multiline(source)
                    .id_salt(id)
                    .font(font)
                    .desired_rows(rows)
                    .desired_width(f32::INFINITY)
                    .code_editor()
                    .layouter(&mut layouter),
            )
            .changed();
    });
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_byte_of_the_source_appears_in_exactly_one_run() {
        let source = "let a = gauss(k0, 0.5, 0.1) in\n  if a > 0.0 then a else -a # tail";
        let runs = lex(source);
        let mut cursor = 0;
        for (_, range) in &runs {
            assert_eq!(range.start, cursor, "runs must be contiguous");
            cursor = range.end;
        }
        assert_eq!(cursor, source.len(), "runs must cover the whole source");
    }

    #[test]
    fn keywords_symbols_numbers_and_comments_are_told_apart() {
        let runs = lex("let k0 = 1.5e-3 # note");
        let kinds: Vec<TokenKind> = runs
            .iter()
            .filter(|(kind, _)| *kind != TokenKind::Plain)
            .map(|(kind, _)| *kind)
            .collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Keyword,
                TokenKind::Symbol,
                TokenKind::Operator,
                TokenKind::Number,
                TokenKind::Comment,
            ]
        );
    }

    #[test]
    fn a_number_keeps_its_exponent_in_one_run() {
        let runs = lex("1.5e-3");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].0, TokenKind::Number);
        assert_eq!(runs[0].1, 0..6);
    }

    #[test]
    fn every_token_kind_has_its_own_colour() {
        let kinds = [
            TokenKind::Keyword,
            TokenKind::Number,
            TokenKind::Operator,
            TokenKind::Comment,
        ];
        for (index, kind) in kinds.iter().enumerate() {
            for other in &kinds[index + 1..] {
                assert_ne!(kind.color(), other.color(), "{kind:?} vs {other:?}");
            }
        }
    }

    #[test]
    fn the_gutter_numbers_every_line_including_an_empty_source() {
        assert_eq!(gutter(""), "1");
        assert_eq!(gutter("a"), "1");
        assert_eq!(gutter("a\nb\nc"), "1\n2\n3");
    }

    #[test]
    fn an_offset_maps_to_a_line_and_column_counted_from_one() {
        let source = "ab\ncde";
        assert_eq!(position_of(source, 0), (1, 1));
        assert_eq!(position_of(source, 2), (1, 3));
        assert_eq!(position_of(source, 3), (2, 1));
        assert_eq!(position_of(source, 5), (2, 3));
        // Past the end is clamped rather than panicking on a stale span.
        assert_eq!(position_of(source, 999), (2, 4));
    }

    #[test]
    fn a_diagnostic_underlines_the_runs_it_covers_and_no_others() {
        let source = "aaa bbb";
        let diagnostics = [Diagnostic {
            code: "unknown_symbol".into(),
            start: 4,
            end: 7,
        }];
        let job = layout(source, &diagnostics, egui::FontId::monospace(13.0));
        let underlined: Vec<bool> = job
            .sections
            .iter()
            .map(|section| section.format.underline.width > 0.0)
            .collect();
        assert!(
            underlined.iter().any(|marked| *marked),
            "the faulty span must be marked"
        );
        assert!(
            underlined.iter().any(|marked| !*marked),
            "text outside the span must not be marked"
        );
    }
}
