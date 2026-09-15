use crate::history::{Entry, History};
use ratatui::widgets::ListState;

pub struct App {
    pub history: History,
    pub visible: Vec<usize>,
    pub list: ListState,
    pub query: String,
    pub searching: bool,
    pub session: Option<String>,
    pub session_origin: Option<std::path::PathBuf>,
    pub oldest_first: bool,
    pub scroll: usize,
    pub help: bool,
    pub status: String,
    pub transcript: Option<Box<App>>,
    pub session_source: Option<std::path::PathBuf>,
    pub session_info: String,
    pub expanded_tools: std::collections::HashSet<usize>,
    pub expanded_groups: std::collections::HashSet<usize>,
    pub groups: std::collections::BTreeMap<usize, std::ops::Range<usize>>,
}

impl App {
    pub fn new(history: History) -> Self {
        let mut app = Self {
            history,
            visible: vec![],
            list: ListState::default(),
            query: String::new(),
            searching: false,
            session: None,
            session_origin: None,
            oldest_first: false,
            scroll: 0,
            help: false,
            status: String::new(),
            transcript: None,
            session_source: None,
            session_info: String::new(),
            expanded_tools: std::collections::HashSet::new(),
            expanded_groups: std::collections::HashSet::new(),
            groups: std::collections::BTreeMap::new(),
        };
        app.filter();
        app
    }

    pub fn selected(&self) -> Option<&Entry> {
        self.list
            .selected()
            .and_then(|i| self.visible.get(i))
            .and_then(|&i| self.history.entries.get(i))
    }

    pub fn from_session(session: crate::session::Session) -> Self {
        let mut app = Self::new(session.history);
        app.session_source = Some(session.path);
        app.session_info = session.info;
        app.filter();
        app
    }

    pub fn selected_group(&self) -> Option<&std::ops::Range<usize>> {
        self.list
            .selected()
            .and_then(|i| self.visible.get(i))
            .and_then(|i| self.groups.get(i))
    }

    pub fn collapse_groups(&mut self) {
        let selected = self
            .list
            .selected()
            .and_then(|i| self.visible.get(i))
            .copied();
        let target = selected.map(|index| {
            self.groups
                .iter()
                .find(|(_, range)| range.contains(&index))
                .map_or(index, |(&id, _)| id)
        });
        self.expanded_groups.clear();
        self.expanded_tools.clear();
        self.filter();
        if let Some(position) = target.and_then(|id| self.visible.iter().position(|&i| i == id)) {
            self.list.select(Some(position));
        }
    }

    pub fn tool_collapsed(&self, index: usize) -> bool {
        self.session_source.is_some()
            && is_tool(&self.history.entries[index])
            && !self.expanded_tools.contains(&index)
    }

    pub fn toggle_tool(&mut self) {
        if let Some(&index) = self.list.selected().and_then(|i| self.visible.get(i))
            && self.groups.contains_key(&index)
        {
            if !self.expanded_groups.remove(&index) {
                self.expanded_groups.insert(index);
            }
            self.filter();
            self.list
                .select(self.visible.iter().position(|&i| i == index));
            return;
        }
        if let Some(&index) = self.list.selected().and_then(|i| self.visible.get(i))
            && self.session_source.is_some()
            && is_tool(&self.history.entries[index])
        {
            if !self.expanded_tools.remove(&index) {
                self.expanded_tools.insert(index);
            }
            self.scroll = 0;
        }
    }

    pub fn filter(&mut self) {
        let query = self.query.to_lowercase();
        self.visible =
            self.history
                .entries
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    self.session.as_ref().is_none_or(|id| {
                        *id == entry.session_id && self.session_origin == entry.source
                    }) && (entry.text.to_lowercase().contains(&query)
                        || entry.session_id.to_lowercase().contains(&query))
                })
                .map(|(i, _)| i)
                .collect();
        if self.oldest_first {
            self.visible.reverse();
        }
        self.groups.clear();
        if self.session_source.is_some() {
            let len = self.history.entries.len();
            let mut start = 0;
            while start < len {
                if !is_tool(&self.history.entries[start]) {
                    start += 1;
                    continue;
                }
                let mut end = start + 1;
                while end < len && is_tool(&self.history.entries[end]) {
                    end += 1;
                }
                if end - start >= 2 {
                    self.groups.insert(len + start, start..end);
                }
                start = end;
            }
            // Search reveals individual matches without requiring group expansion.
            if self.query.is_empty() {
                let mut rows = Vec::new();
                let mut seen = std::collections::HashSet::new();
                for &index in &self.visible {
                    if let Some((&id, _)) =
                        self.groups.iter().find(|(_, range)| range.contains(&index))
                    {
                        if seen.insert(id) {
                            rows.push(id);
                        }
                        if self.expanded_groups.contains(&id) {
                            rows.push(index);
                        }
                    } else {
                        rows.push(index);
                    }
                }
                self.visible = rows;
            }
        }
        self.list = ListState::default().with_selected((!self.visible.is_empty()).then_some(0));
        self.scroll = 0;
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let next = self
            .list
            .selected()
            .unwrap_or(0)
            .saturating_add_signed(delta)
            .min(self.visible.len() - 1);
        self.list.select(Some(next));
        self.scroll = 0;
    }

    pub fn toggle_session(&mut self) {
        let selected = self
            .list
            .selected()
            .and_then(|position| self.visible.get(position))
            .copied();
        let row = self
            .list
            .selected()
            .unwrap_or(0)
            .saturating_sub(self.list.offset());
        let scroll = self.scroll;
        if self.session.is_some() {
            self.session = None;
            self.session_origin = None;
        } else if let Some(entry) = self.selected() {
            let (id, source) = (entry.session_id.clone(), entry.source.clone());
            self.session = Some(id);
            self.session_origin = source;
        }
        self.filter();
        if let Some(position) =
            selected.and_then(|index| self.visible.iter().position(|&i| i == index))
        {
            self.list.select(Some(position));
            *self.list.offset_mut() = position.saturating_sub(row);
            self.scroll = scroll;
        }
    }
}

pub fn is_tool(entry: &Entry) -> bool {
    entry.session_id.starts_with("TOOL ·") || entry.session_id.starts_with("RESULT ·")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_toggle_preserves_the_selected_record_in_both_directions() {
        let mut a = app();
        a.move_selection(2);
        a.scroll = 3;
        a.toggle_session();
        assert_eq!(a.visible, vec![0, 2]);
        assert_eq!(a.list.selected(), Some(1));
        assert_eq!(a.scroll, 3);
        a.toggle_session();
        assert_eq!(a.list.selected(), Some(2));
        assert_eq!(a.selected().unwrap().ts, 1);
        // Preserve the current record even after moving within the filtered list.
        a.toggle_session();
        a.move_selection(-1);
        a.toggle_session();
        assert_eq!(a.list.selected(), Some(0));
        a.query = "rust".into();
        a.oldest_first = true;
        a.filter();
        a.move_selection(1);
        a.toggle_session();
        a.toggle_session();
        assert_eq!(a.list.selected(), Some(1));
        assert_eq!(a.selected().unwrap().ts, 3);
    }
    #[test]
    fn session_filter_keeps_sources_separate() {
        let mut a = app();
        a.history.entries[0].source = Some(".codex/history.jsonl".into());
        a.history.entries[2].source = Some(".codex-beta/history.jsonl".into());
        a.toggle_session();
        assert_eq!(a.visible, vec![0]);
        a.toggle_session();
        assert_eq!(a.visible, vec![0, 1, 2]);
    }
    #[test]
    fn groups_tools_with_nested_folding_search_and_reverse_order() {
        let mut a = App::new(History {
            entries: vec![
                Entry {
                    source: None,
                    session_id: "USER".into(),
                    ts: 1,
                    text: "hello".into(),
                },
                Entry {
                    source: None,
                    session_id: "TOOL · shell".into(),
                    ts: 2,
                    text: "Completed\nneedle".into(),
                },
                Entry {
                    source: None,
                    session_id: "TOOL · search".into(),
                    ts: 3,
                    text: "Failed\noutput".into(),
                },
                Entry {
                    source: None,
                    session_id: "ASSISTANT".into(),
                    ts: 4,
                    text: "done".into(),
                },
            ],
            skipped: 0,
        });
        a.session_source = Some("demo".into());
        a.filter();
        assert_eq!(a.visible, vec![0, 5, 3]);
        a.move_selection(1);
        assert_eq!(a.selected_group(), Some(&(1..3)));
        assert!(a.selected().is_none());
        a.toggle_tool();
        assert_eq!(a.visible, vec![0, 5, 1, 2, 3]);
        a.move_selection(1);
        a.toggle_tool();
        assert!(!a.tool_collapsed(1));
        a.collapse_groups();
        assert_eq!(a.visible, vec![0, 5, 3]);
        assert_eq!(a.list.selected(), Some(1));
        assert!(a.tool_collapsed(1));
        a.query = "needle".into();
        a.filter();
        assert_eq!(a.visible, vec![1]);
        a.query.clear();
        a.oldest_first = true;
        a.filter();
        assert_eq!(a.visible, vec![3, 5, 0]);
        a.move_selection(1);
        a.toggle_tool();
        assert_eq!(a.visible, vec![3, 5, 2, 1, 0]);
    }
    fn app() -> App {
        App::new(History {
            entries: vec![
                Entry {
                    source: None,
                    session_id: "a".into(),
                    ts: 3,
                    text: "RUST 世界".into(),
                },
                Entry {
                    source: None,
                    session_id: "b".into(),
                    ts: 2,
                    text: "other".into(),
                },
                Entry {
                    source: None,
                    session_id: "a".into(),
                    ts: 1,
                    text: "rust again".into(),
                },
            ],
            skipped: 0,
        })
    }
    #[test]
    fn search_session_sort_and_navigation() {
        let mut a = app();
        a.query = "rust".into();
        a.filter();
        assert_eq!(a.visible, vec![0, 2]);
        a.toggle_session();
        assert_eq!(a.session.as_deref(), Some("a"));
        a.oldest_first = true;
        a.filter();
        assert_eq!(a.selected().unwrap().ts, 1);
        a.move_selection(100);
        assert_eq!(a.selected().unwrap().ts, 3);
        a.query = "missing".into();
        a.filter();
        a.move_selection(-1);
        assert!(a.selected().is_none());
    }
}
