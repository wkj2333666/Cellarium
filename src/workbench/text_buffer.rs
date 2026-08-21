#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextBuffer {
    text: String,
    cursor: usize,
    preferred_column: Option<usize>,
}
impl TextBuffer {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
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
    pub fn replace(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.preferred_column = None;
    }
    pub fn insert_str(&mut self, value: &str) {
        self.text.insert_str(self.cursor, value);
        self.cursor += value.len();
        self.preferred_column = None;
    }
    pub fn insert_char(&mut self, value: char) {
        self.text.insert(self.cursor, value);
        self.cursor += value.len_utf8();
        self.preferred_column = None;
    }
    pub fn newline(&mut self) {
        self.insert_char('\n');
    }
    pub fn backspace(&mut self) -> bool {
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
        if self.cursor == self.text.len() {
            return false;
        }
        let next = self.cursor + self.text[self.cursor..].chars().next().unwrap().len_utf8();
        self.text.replace_range(self.cursor..next, "");
        true
    }
    pub fn move_end(&mut self) {
        self.cursor = self.line_end(self.cursor);
        self.preferred_column = None;
    }
    pub fn move_home(&mut self) {
        self.cursor = self.line_start(self.cursor);
        self.preferred_column = None;
    }
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
        }
        self.preferred_column = None;
    }
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor += self.text[self.cursor..].chars().next().unwrap().len_utf8();
        }
        self.preferred_column = None;
    }
    pub fn move_vertical(&mut self, delta: isize) {
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
    fn line_start(&self, at: usize) -> usize {
        self.text[..at].rfind('\n').map_or(0, |index| index + 1)
    }
    fn line_end(&self, at: usize) -> usize {
        self.text[at..]
            .find('\n')
            .map_or(self.text.len(), |offset| at + offset)
    }
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
}
