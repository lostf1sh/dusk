use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

fn char_count(s: &str) -> usize {
    s.chars().count()
}

/// Byte offset of the character at `char_idx`, or `s.len()` if `char_idx` is past the end.
fn byte_at_char_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or_else(|| s.len())
}

/// A simple single-line text input.
pub struct TextInput<'a> {
    pub query: &'a str,
    /// Cursor position in **Unicode scalar values** (user-perceived characters).
    pub cursor: usize,
    pub label: &'a str,
}

impl Widget for TextInput<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 || area.width < 3 {
            return;
        }

        let label_style = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let text_style = Style::default().fg(Color::White);
        let cursor_style = Style::default().fg(Color::Black).bg(Color::White);

        // Render label
        let label_len = self.label.len() as u16;
        buf.set_string(area.x, area.y, self.label, label_style);

        // Render text
        let text_start = area.x + label_len;
        let available = (area.width - label_len) as usize;

        // Calculate visible window of text
        let visible_start = if self.cursor > available.saturating_sub(1) {
            self.cursor - available + 1
        } else {
            0
        };
        let visible_text: String = self
            .query
            .chars()
            .skip(visible_start)
            .take(available)
            .collect();
        buf.set_string(text_start, area.y, &visible_text, text_style);

        // Render cursor
        let cursor_screen_pos = text_start + (self.cursor - visible_start) as u16;
        if cursor_screen_pos < area.x + area.width {
            let cursor_char = self.query.chars().nth(self.cursor).unwrap_or(' ');
            buf.set_string(
                cursor_screen_pos,
                area.y,
                cursor_char.to_string(),
                cursor_style,
            );
        }
    }
}

/// State for a text input: the query string and cursor position (character index).
#[derive(Debug, Clone)]
pub struct TextInputState {
    pub query: String,
    pub cursor: usize,
}

impl TextInputState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
        }
    }

    pub fn insert(&mut self, ch: char) {
        let byte = byte_at_char_index(&self.query, self.cursor);
        self.query.insert(byte, ch);
        self.cursor += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        self.cursor -= 1;
        let byte = byte_at_char_index(&self.query, self.cursor);
        let rest = &self.query[byte..];
        let ch = rest.chars().next().expect("cursor on valid char boundary");
        let len = ch.len_utf8();
        self.query.drain(byte..byte + len);
    }

    #[allow(dead_code)]
    pub fn delete(&mut self) {
        if self.cursor >= char_count(&self.query) {
            return;
        }
        let byte = byte_at_char_index(&self.query, self.cursor);
        let rest = &self.query[byte..];
        let ch = rest.chars().next().expect("cursor on valid char boundary");
        let len = ch.len_utf8();
        self.query.drain(byte..byte + len);
    }

    pub fn move_left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_right(&mut self) {
        let max = char_count(&self.query);
        self.cursor = (self.cursor + 1).min(max);
    }

    #[allow(dead_code)]
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }

    #[allow(dead_code)]
    pub fn move_end(&mut self) {
        self.cursor = char_count(&self.query);
    }

    #[allow(dead_code)]
    pub fn clear(&mut self) {
        self.query.clear();
        self.cursor = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_input() {
        let mut state = TextInputState::new();
        state.insert('a');
        state.insert('b');
        state.insert('c');
        assert_eq!(state.query, "abc");
        assert_eq!(state.cursor, 3);

        state.backspace();
        assert_eq!(state.query, "ab");
        assert_eq!(state.cursor, 2);
    }

    #[test]
    fn test_cursor_movement() {
        let mut state = TextInputState::new();
        state.insert('h');
        state.insert('i');
        state.move_left();
        assert_eq!(state.cursor, 1);
        state.insert('X');
        assert_eq!(state.query, "hXi");

        state.move_home();
        assert_eq!(state.cursor, 0);
        state.move_end();
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn test_unicode_insert_and_backspace() {
        let mut state = TextInputState::new();
        state.insert('é');
        state.insert('a');
        assert_eq!(state.query, "éa");
        assert_eq!(state.cursor, 2);
        state.backspace();
        assert_eq!(state.query, "é");
        assert_eq!(state.cursor, 1);
        state.backspace();
        assert_eq!(state.query, "");
        assert_eq!(state.cursor, 0);
    }

    #[test]
    fn test_unicode_movement() {
        let mut state = TextInputState::new();
        state.query = "αβγ".to_string();
        state.cursor = 3;
        state.move_left();
        assert_eq!(state.cursor, 2);
        state.move_left();
        assert_eq!(state.cursor, 1);
        state.move_left();
        assert_eq!(state.cursor, 0);
        state.move_right();
        assert_eq!(state.cursor, 1);
    }
}
