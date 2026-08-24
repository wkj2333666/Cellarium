#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextSelection {
    pub anchor: usize,
    pub cursor: usize,
}

impl TextSelection {
    pub fn range(self) -> std::ops::Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBuffer {
    text: String,
    cursor: usize,
    selection_anchor: Option<usize>,
    preferred_column: Option<usize>,
}
impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
            selection_anchor: None,
            preferred_column: None,
        }
    }
    pub fn as_str(&self) -> &str {
        &self.text
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn cursor_is_char_boundary(&self) -> bool {
        self.text.is_char_boundary(self.cursor)
    }
    pub fn selection(&self) -> Option<TextSelection> {
        self.selection_anchor
            .filter(|anchor| *anchor != self.cursor)
            .map(|anchor| TextSelection {
                anchor,
                cursor: self.cursor,
            })
    }
    pub fn selected_text(&self) -> Option<&str> {
        let range = self.selection()?.range();
        Some(&self.text[range])
    }
    pub fn begin_selection(&mut self) {
        self.selection_anchor = Some(self.cursor);
    }
    pub fn select_all(&mut self) {
        if self.text.is_empty() {
            self.selection_anchor = None;
            self.cursor = 0;
        } else {
            self.selection_anchor = Some(0);
            self.cursor = self.text.len();
        }
        self.preferred_column = None;
    }
    pub fn delete_to_line_start(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        let start = self.line_start(self.cursor);
        if start == self.cursor {
            return false;
        }
        self.text.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.preferred_column = None;
        true
    }
    pub fn set_cursor_line_column(&mut self, line: usize, column: usize) {
        self.set_cursor_line_column_inner(line, column, false);
    }
    pub fn set_cursor_line_column_extending(&mut self, line: usize, column: usize) {
        self.set_cursor_line_column_inner(line, column, true);
    }
    fn set_cursor_line_column_inner(&mut self, line: usize, column: usize, extend: bool) {
        self.prepare_selection(extend);
        let start = self
            .text
            .match_indices('\n')
            .nth(line.saturating_sub(1))
            .map_or_else(
                || if line == 0 { 0 } else { self.text.len() },
                |(index, _)| index + 1,
            );
        let end = self.text[start..]
            .find('\n')
            .map_or(self.text.len(), |offset| start + offset);
        self.cursor = byte_at_column(&self.text, start, end, column);
        self.preferred_column = None;
    }
    pub fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.selection_anchor = None;
        self.preferred_column = None;
    }
    pub fn insert_str(&mut self, value: &str) {
        self.delete_selection();
        self.text.insert_str(self.cursor, value);
        self.cursor += value.len();
        self.preferred_column = None;
    }
    pub fn insert_char(&mut self, value: char) {
        self.delete_selection();
        self.text.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.preferred_column = None;
    }
    pub fn newline(&mut self) {
        self.insert_char('\n');
    }
    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let previous = self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index);
        self.text.replace_range(previous..self.cursor, "");
        self.cursor = previous;
        self.preferred_column = None;
        true
    }
    pub fn delete(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == self.text.len() {
            return false;
        }
        let next = self.cursor + self.text[self.cursor..].chars().next().unwrap().len_utf8();
        self.text.replace_range(self.cursor..next, "");
        true
    }
    pub fn move_end(&mut self) {
        self.selection_anchor = None;
        self.cursor = self.line_end(self.cursor);
        self.preferred_column = None;
    }
    pub fn move_home(&mut self) {
        self.selection_anchor = None;
        self.cursor = self.line_start(self.cursor);
        self.preferred_column = None;
    }
    pub fn move_left(&mut self) {
        self.selection_anchor = None;
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
        }
        self.preferred_column = None;
    }
    pub fn move_right(&mut self) {
        self.selection_anchor = None;
        if self.cursor < self.text.len() {
            self.cursor += self.text[self.cursor..].chars().next().unwrap().len_utf8();
        }
        self.preferred_column = None;
    }
    pub fn move_vertical(&mut self, delta: isize) {
        self.selection_anchor = None;
        let start = self.line_start(self.cursor);
        let column = self.text[start..self.cursor].chars().count();
        let column = *self.preferred_column.get_or_insert(column);
        if delta < 0 {
            if start == 0 {
                return;
            }
            let previous_end = start - 1;
            let previous_start = self.line_start(previous_end);
            self.cursor = byte_at_column(&self.text, previous_start, previous_end, column);
        } else {
            let end = self.line_end(self.cursor);
            if end == self.text.len() {
                return;
            }
            let next_start = end + 1;
            let next_end = self.line_end(next_start);
            self.cursor = byte_at_column(&self.text, next_start, next_end, column);
        }
    }
    pub fn move_word_right(&mut self, extend: bool) {
        self.prepare_selection(extend);
        let mut cursor = self.cursor;
        let first_is_word = self.text[cursor..]
            .chars()
            .next()
            .is_some_and(is_word_character);
        while cursor < self.text.len() {
            let character = self.text[cursor..].chars().next().unwrap();
            if is_word_character(character) != first_is_word {
                break;
            }
            cursor += character.len_utf8();
        }
        if !first_is_word {
            while cursor < self.text.len() {
                let character = self.text[cursor..].chars().next().unwrap();
                if !is_word_character(character) {
                    break;
                }
                cursor += character.len_utf8();
            }
        }
        self.cursor = cursor;
        self.preferred_column = None;
    }
    pub fn move_word_left(&mut self, extend: bool) {
        self.prepare_selection(extend);
        let mut cursor = self.cursor;
        while cursor > 0 {
            let (previous, character) = self.text[..cursor].char_indices().next_back().unwrap();
            if !character.is_whitespace() {
                break;
            }
            cursor = previous;
        }
        while cursor > 0 {
            let (previous, character) = self.text[..cursor].char_indices().next_back().unwrap();
            if !is_word_character(character) {
                break;
            }
            cursor = previous;
        }
        self.cursor = cursor;
        self.preferred_column = None;
    }
    pub fn move_left_extending(&mut self) {
        self.prepare_selection(true);
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
        }
        self.preferred_column = None;
    }
    pub fn move_right_extending(&mut self) {
        self.prepare_selection(true);
        if self.cursor < self.text.len() {
            self.cursor += self.text[self.cursor..].chars().next().unwrap().len_utf8();
        }
        self.preferred_column = None;
    }
    fn prepare_selection(&mut self, extend: bool) {
        if extend {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
    }
    fn delete_selection(&mut self) -> bool {
        let Some(selection) = self.selection() else {
            self.selection_anchor = None;
            return false;
        };
        let range = selection.range();
        self.cursor = range.start;
        self.text.replace_range(range, "");
        self.selection_anchor = None;
        self.preferred_column = None;
        true
    }
    fn line_start(&self, at: usize) -> usize {
        self.text[..at].rfind('\n').map_or(0, |index| index + 1)
    }
    fn line_end(&self, at: usize) -> usize {
        self.text[at..]
            .find('\n')
            .map_or(self.text.len(), |offset| at + offset)
    }
}
fn is_word_character(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}
fn byte_at_column(text: &str, start: usize, end: usize, column: usize) -> usize {
    text[start..end]
        .char_indices()
        .nth(column)
        .map_or(end, |(offset, _)| start + offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_utf8_boundaries() {
        let mut buffer = TextBuffer::new("// 生长\ninner");
        buffer.move_end();
        buffer.insert_str(" + self");
        assert_eq!(buffer.as_str(), "// 生长\ninner + self");
        assert!(buffer.cursor_is_char_boundary());
        buffer.backspace();
        assert_eq!(buffer.as_str(), "// 生长\ninner + sel");
    }

    #[test]
    fn mouse_style_line_column_placement_respects_utf8_boundaries() {
        let mut buffer = TextBuffer::new("alpha\n生长 + self");
        buffer.set_cursor_line_column(1, 2);
        buffer.insert_char('X');
        assert_eq!(buffer.as_str(), "alpha\n生长X + self");
        assert!(buffer.cursor_is_char_boundary());
    }

    #[test]
    fn utf8_selection_replaces_the_selected_span_and_tracks_anchor() {
        let mut buffer = TextBuffer::new("alpha 生长 omega");
        buffer.set_cursor_line_column(0, 6);
        buffer.begin_selection();
        buffer.set_cursor_line_column_extending(0, 8);
        assert_eq!(buffer.selected_text(), Some("生长"));
        buffer.insert_str("rate");
        assert_eq!(buffer.as_str(), "alpha rate omega");
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn word_movement_and_shift_extension_are_visible_state() {
        let mut buffer = TextBuffer::new("let value = kernel + self");
        buffer.move_home();
        buffer.move_word_right(false);
        assert_eq!(&buffer.as_str()[..buffer.cursor()], "let");
        buffer.move_word_right(true);
        assert_eq!(buffer.selected_text(), Some(" value"));
        buffer.move_word_left(false);
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn select_all_replaces_the_complete_utf8_program() {
        let mut buffer = TextBuffer::new("if potential > 0.5 { 生长 } else { self }");

        buffer.select_all();
        assert_eq!(
            buffer.selected_text(),
            Some("if potential > 0.5 { 生长 } else { self }")
        );
        buffer.insert_str("potential - self");

        assert_eq!(buffer.as_str(), "potential - self");
        assert_eq!(buffer.selection(), None);
    }

    #[test]
    fn delete_to_line_start_removes_only_the_current_line_prefix() {
        let mut buffer = TextBuffer::new("let x = potential;\nself + x");

        assert!(buffer.delete_to_line_start());

        assert_eq!(buffer.as_str(), "let x = potential;\n");
        assert_eq!(buffer.cursor(), "let x = potential;\n".len());
    }
}
