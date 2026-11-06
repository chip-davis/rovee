/// A single-line text field with a cursor, for the rename popup.
pub struct TextInput {
    pub value: String,
    /// Cursor position in characters, not bytes.
    pub cursor: usize,
}

impl TextInput {
    pub fn new(value: String) -> Self {
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    fn byte_index(&self) -> usize {
        self.value
            .char_indices()
            .map(|(i, _)| i)
            .nth(self.cursor)
            .unwrap_or(self.value.len())
    }

    pub fn insert(&mut self, c: char) {
        let i = self.byte_index();
        self.value.insert(i, c);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let before = self.value.chars().take(self.cursor - 1);
        let after = self.value.chars().skip(self.cursor);
        self.value = before.chain(after).collect();
        self.cursor -= 1;
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.value.chars().count());
    }
}
