use std::{cell::RefCell, rc::Rc};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Default)]
pub struct Editor {
    pub history: Rc<RefCell<Vec<String>>>,
    cursor: usize,
    start: usize,
    history_position: Option<usize>,
    scratch: String,
}

impl Editor {
    pub fn begin(&mut self, query: &str) {
        self.cursor = query.len();
        self.start = 0;
        self.history_position = None;
        self.scratch = query.to_owned();
    }
    pub fn left(&mut self, query: &str) {
        self.cursor = query[..self.cursor]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(index, _)| index);
    }
    pub fn right(&mut self, query: &str) {
        if let Some(grapheme) = query[self.cursor..].graphemes(true).next() {
            self.cursor += grapheme.len();
        }
    }
    pub fn home(&mut self) {
        self.cursor = 0;
    }
    pub fn end(&mut self, query: &str) {
        self.cursor = query.len();
    }
    pub fn insert(&mut self, query: &mut String, text: &str) {
        let text: String = text
            .chars()
            .filter_map(|c| {
                if c == '\n' || c == '\r' || c == '\t' {
                    Some(' ')
                } else if c.is_control() {
                    None
                } else {
                    Some(c)
                }
            })
            .collect();
        query.insert_str(self.cursor, &text);
        self.cursor += text.len();
        // Joining characters (e.g. combining accents) may merge with neighbors.
        self.cursor = query
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|&index| index >= self.cursor)
            .unwrap_or(query.len());
        self.history_position = None;
        self.normalize_start(query);
    }
    pub fn backspace(&mut self, query: &mut String) {
        let end = self.cursor;
        self.left(query);
        query.replace_range(self.cursor..end, "");
        self.history_position = None;
        self.normalize_start(query);
    }
    pub fn delete(&mut self, query: &mut String) {
        let start = self.cursor;
        self.right(query);
        query.replace_range(start..self.cursor, "");
        self.cursor = start;
        self.history_position = None;
        self.normalize_start(query);
    }
    pub fn delete_word(&mut self, query: &mut String) {
        let end = self.cursor;
        let mut start = end;
        let mut word = false;
        for (index, grapheme) in query[..end].grapheme_indices(true).rev() {
            let space = grapheme.chars().all(char::is_whitespace);
            if word && space {
                break;
            }
            if !space {
                word = true;
            }
            start = index;
        }
        query.replace_range(start..end, "");
        self.cursor = start;
        self.history_position = None;
        self.normalize_start(query);
    }
    pub fn clear(&mut self, query: &mut String) {
        query.clear();
        self.cursor = 0;
        self.start = 0;
        self.history_position = None;
    }
    pub fn recall(&mut self, query: &mut String, previous: bool) {
        let history = self.history.borrow();
        if history.is_empty() {
            return;
        }
        if previous {
            if self.history_position.is_none() {
                self.scratch = query.clone();
            }
            let position = self
                .history_position
                .map_or(history.len() - 1, |index| index.saturating_sub(1));
            *query = history[position].clone();
            self.history_position = Some(position);
        } else if let Some(position) = self.history_position {
            if position + 1 < history.len() {
                *query = history[position + 1].clone();
                self.history_position = Some(position + 1);
            } else {
                *query = self.scratch.clone();
                self.history_position = None;
            }
        }
        self.cursor = query.len();
        self.start = 0;
    }
    pub fn remember(&mut self, query: &str) {
        if query.is_empty() {
            return;
        }
        let mut history = self.history.borrow_mut();
        history.retain(|previous| previous != query);
        history.push(query.to_owned());
        if history.len() > 20 {
            history.remove(0);
        }
    }
    fn normalize_start(&mut self, query: &str) {
        self.cursor = query
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .find(|&index| index >= self.cursor)
            .unwrap_or(query.len());
        self.start = query
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .take_while(|&index| index <= self.start.min(self.cursor))
            .last()
            .unwrap_or(0);
    }
    /// Return a grapheme-safe visible window and the caret's terminal column.
    pub fn view(&mut self, query: &str, width: usize) -> (String, usize) {
        if width == 0 {
            return (String::new(), 0);
        }
        self.normalize_start(query);
        if self.cursor < self.start {
            self.start = self.cursor;
        }
        while query[self.start..self.cursor].width() >= width {
            let Some(first) = query[self.start..self.cursor].graphemes(true).next() else {
                break;
            };
            self.start += first.len();
        }
        let mut text = String::new();
        let mut used = 0;
        for grapheme in query[self.start..].graphemes(true) {
            let size = grapheme.width();
            if used + size > width {
                break;
            }
            text.push_str(grapheme);
            used += size;
        }
        (text, query[self.start..self.cursor].width().min(width - 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_in_the_middle_without_splitting_unicode_graphemes() {
        let mut editor = Editor::default();
        let mut query = "a👩‍💻e\u{301}中".to_owned();
        editor.begin(&query);
        editor.left(&query);
        editor.backspace(&mut query);
        assert_eq!(query, "a👩‍💻中");
        editor.left(&query);
        editor.delete(&mut query);
        assert_eq!(query, "a中");
        editor.insert(&mut query, "文");
        assert_eq!(query, "a文中");
        editor.home();
        editor.insert(&mut query, "start ");
        assert_eq!(query, "start a文中");
        editor.end(&query);
        editor.delete_word(&mut query);
        assert_eq!(query, "start ");
    }
    #[test]
    fn history_restores_the_draft_and_is_bounded_and_shared() {
        let mut editor = Editor::default();
        for index in 0..25 {
            editor.remember(&index.to_string());
        }
        assert_eq!(editor.history.borrow().len(), 20);
        let mut other = Editor {
            history: editor.history.clone(),
            ..Default::default()
        };
        let mut query = "draft".to_owned();
        other.begin(&query);
        other.recall(&mut query, true);
        assert_eq!(query, "24");
        other.recall(&mut query, true);
        assert_eq!(query, "23");
        other.recall(&mut query, false);
        other.recall(&mut query, false);
        assert_eq!(query, "draft");
        other.remember("24");
        assert_eq!(editor.history.borrow().len(), 20);
    }
    #[test]
    fn caret_stays_visible_and_paste_is_single_line() {
        let mut editor = Editor::default();
        let mut query = "中文abc👩‍💻".to_owned();
        editor.begin(&query);
        let (text, cursor) = editor.view(&query, 5);
        assert!(text.width() <= 5 && cursor < 5);
        editor.home();
        assert_eq!(editor.view(&query, 5).1, 0);
        editor.insert(&mut query, "x\n\u{1b}y\t");
        assert!(query.starts_with("x y "));
    }
}
