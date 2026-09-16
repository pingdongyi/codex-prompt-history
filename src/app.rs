use crate::history::{Entry, History};
use ratatui::widgets::ListState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    List,
    Detail,
}

pub struct App {
    pub history: History,
    pub visible: Vec<usize>,
    pub list: ListState,
    pub query: String,
    pub searching: bool,
    pub session: Option<String>,
    pub session_origin: Option<std::path::PathBuf>,
    pub source_filter: Option<std::path::PathBuf>,
    pub focus: Focus,
    pub detail_page_size: usize,
    pub detail_max_scroll: usize,
    pub matched_count: usize,
    selection_anchor: Option<(usize, usize)>,
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
            source_filter: None,
            focus: Focus::List,
            detail_page_size: 1,
            detail_max_scroll: 0,
            matched_count: 0,
            selection_anchor: None,
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
        let current = self
            .list
            .selected()
            .and_then(|i| self.visible.get(i))
            .copied();
        let selected = current.or(self.selection_anchor.map(|(index, _)| index));
        let old_position = self.list.selected().unwrap_or(0);
        let row = old_position.saturating_sub(self.list.offset());
        let scroll = if current.is_some() {
            self.scroll
        } else {
            self.selection_anchor
                .map_or(self.scroll, |(_, scroll)| scroll)
        };
        let query = self.query.to_lowercase();
        self.visible = self
            .history
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                self.source_filter
                    .as_ref()
                    .is_none_or(|source| entry.source.as_ref() == Some(source))
                    && self.session.as_ref().is_none_or(|id| {
                        *id == entry.session_id && self.session_origin == entry.source
                    })
                    && (entry.text.to_lowercase().contains(&query)
                        || entry.session_id.to_lowercase().contains(&query))
            })
            .map(|(i, _)| i)
            .collect();
        self.matched_count = self.visible.len();
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
        let restored = selected.and_then(|index| self.visible_position(index));
        let position = restored.or_else(|| {
            (!self.visible.is_empty()).then(|| old_position.min(self.visible.len() - 1))
        });
        self.list = ListState::default()
            .with_selected(position)
            .with_offset(position.unwrap_or(0).saturating_sub(row));
        self.scroll = if restored.is_some() { scroll } else { 0 };
        if let Some(index) = position.and_then(|p| self.visible.get(p)).copied() {
            self.selection_anchor = Some((index, self.scroll));
        } else if let Some(index) = selected {
            self.selection_anchor = Some((index, scroll));
        }
    }

    fn visible_position(&self, index: usize) -> Option<usize> {
        self.visible.iter().position(|&i| i == index).or_else(|| {
            self.groups
                .iter()
                .find(|(_, range)| range.contains(&index))
                .and_then(|(&group, _)| self.visible.iter().position(|&i| i == group))
        })
    }

    pub fn replace_history(&mut self, history: History) {
        let index = self
            .list
            .selected()
            .and_then(|i| self.visible.get(i))
            .copied();
        let selected_group = index.is_some_and(|id| self.groups.contains_key(&id));
        let entry_index =
            index.and_then(|i| self.groups.get(&i).map(|range| range.start).or(Some(i)));
        let selected = entry_index
            .and_then(|i| self.history.entries.get(i))
            .cloned();
        let occurrence = selected
            .as_ref()
            .zip(entry_index)
            .map_or(0, |(entry, index)| {
                self.history.entries[..index]
                    .iter()
                    .filter(|e| *e == entry)
                    .count()
            });
        let row = self
            .list
            .selected()
            .unwrap_or(0)
            .saturating_sub(self.list.offset());
        let scroll = self.scroll;
        // Remap expanded tools and groups by their contents when new records arrive.
        let expanded: Vec<Entry> = self
            .expanded_tools
            .iter()
            .filter_map(|&i| self.history.entries.get(i).cloned())
            .collect();
        let groups: Vec<Entry> = self
            .expanded_groups
            .iter()
            .filter_map(|id| self.groups.get(id))
            .filter_map(|r| self.history.entries.get(r.start).cloned())
            .collect();
        self.history = history;
        self.expanded_tools = self
            .history
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| expanded.contains(e))
            .map(|(i, _)| i)
            .collect();
        self.expanded_groups.clear();
        self.visible.clear();
        self.selection_anchor = None;
        self.filter();
        self.expanded_groups = self
            .groups
            .iter()
            .filter(|(_, r)| groups.contains(&self.history.entries[r.start]))
            .map(|(&id, _)| id)
            .collect();
        self.filter();
        if let Some(index) = selected.as_ref().and_then(|entry| {
            self.history
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| *e == entry)
                .nth(occurrence)
                .map(|(i, _)| i)
        }) && let Some(position) = if selected_group {
            self.groups
                .iter()
                .find(|(_, range)| range.contains(&index))
                .and_then(|(&id, _)| self.visible.iter().position(|&i| i == id))
                .or_else(|| self.visible_position(index))
        } else {
            self.visible_position(index)
        } {
            self.list.select(Some(position));
            *self.list.offset_mut() = position.saturating_sub(row);
            self.scroll = scroll;
        }
    }

    pub fn toggle_focus(&mut self) {
        self.focus = if self.focus == Focus::List {
            Focus::Detail
        } else {
            Focus::List
        };
    }

    pub fn navigate(&mut self, delta: isize) {
        if self.focus == Focus::List {
            self.move_selection(delta);
        } else {
            self.scroll = self
                .scroll
                .saturating_add_signed(delta)
                .min(self.detail_max_scroll);
        }
    }

    pub fn page(&mut self, direction: isize) {
        let size = if self.focus == Focus::Detail {
            self.detail_page_size
        } else {
            10
        };
        self.navigate(direction * size.max(1) as isize);
    }

    pub fn clear_one_filter(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
        } else if self.session.is_some() {
            self.session = None;
            self.session_origin = None;
        } else if self.source_filter.is_some() {
            self.source_filter = None;
        } else {
            return false;
        }
        self.filter();
        true
    }

    pub fn cycle_source(&mut self) {
        let sources: std::collections::BTreeSet<_> = self
            .history
            .entries
            .iter()
            .filter_map(|e| e.source.clone())
            .collect();
        self.source_filter = match &self.source_filter {
            None => sources.first().cloned(),
            Some(current) => sources.iter().find(|source| *source > current).cloned(),
        };
        self.session = None;
        self.session_origin = None;
        self.filter();
    }

    pub fn filter_summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.query.is_empty() {
            parts.push(format!("Search: {} [x]", self.query));
        }
        if let Some(session) = &self.session {
            parts.push(format!(
                "Session: {} [s]",
                session.chars().take(8).collect::<String>()
            ));
        }
        if self.session_source.is_none() {
            parts.push(format!(
                "Source: {} [t]",
                self.source_filter
                    .as_deref()
                    .map(crate::history::source_name)
                    .unwrap_or_else(|| "all".into())
            ));
        }
        if parts.is_empty() {
            "Filters: none".into()
        } else {
            parts.join(" · ")
        }
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
    fn filtering_and_reload_keep_the_record_and_scroll() {
        let mut a = app();
        a.move_selection(2);
        a.scroll = 7;
        a.query = "rust".into();
        a.filter();
        assert_eq!(a.selected().unwrap().ts, 1);
        assert_eq!(a.scroll, 7);
        a.oldest_first = true;
        a.filter();
        assert_eq!(a.selected().unwrap().ts, 1);
        a.query = "no matches".into();
        a.filter();
        assert!(a.selected().is_none());
        a.scroll = 0; // An empty preview is drawn.
        a.query.clear();
        a.filter();
        assert_eq!(a.selected().unwrap().ts, 1);
        assert_eq!(a.scroll, 7);
        let mut replacement = app().history;
        replacement.entries.insert(
            0,
            Entry {
                source: None,
                session_id: "new".into(),
                ts: 4,
                text: "newest".into(),
            },
        );
        a.replace_history(replacement);
        assert_eq!(a.selected().unwrap().ts, 1);
        assert_eq!(a.scroll, 7);
    }

    #[test]
    fn navigation_moves_only_the_focused_pane() {
        let mut a = app();
        a.detail_max_scroll = 100;
        a.detail_page_size = 12;
        a.toggle_focus();
        a.navigate(1);
        a.page(1);
        assert_eq!(a.scroll, 13);
        assert_eq!(a.list.selected(), Some(0));
        a.navigate(isize::MAX);
        assert_eq!(a.scroll, 100);
        a.navigate(isize::MIN);
        assert_eq!(a.scroll, 0);
        a.toggle_focus();
        a.navigate(1);
        assert_eq!(a.list.selected(), Some(1));
    }

    #[test]
    fn source_cycle_and_escape_clear_filters_in_order() {
        let mut a = app();
        for (entry, source) in
            a.history
                .entries
                .iter_mut()
                .zip([".codex", ".codex-beta", ".codex-gamma"])
        {
            entry.source = Some(std::path::Path::new(source).join("history.jsonl"));
        }
        a.cycle_source();
        assert_eq!(a.visible, vec![0]);
        assert!(a.filter_summary().contains("Source: alpha"));
        a.cycle_source();
        assert_eq!(a.visible, vec![1]);
        a.cycle_source();
        assert_eq!(a.visible, vec![2]);
        a.toggle_session();
        a.query = "rust".into();
        a.filter();
        assert!(a.clear_one_filter());
        assert!(a.query.is_empty());
        assert!(a.session.is_some());
        assert!(a.clear_one_filter());
        assert!(a.session.is_none());
        assert!(a.source_filter.is_some());
        assert!(a.clear_one_filter());
        assert!(a.source_filter.is_none());
        assert!(!a.clear_one_filter());
        assert_eq!(a.visible.len(), 3);
    }
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
        assert_eq!(a.selected_group(), Some(&(1..3)));
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
        assert_eq!(a.selected().unwrap().ts, 3);
        a.move_selection(100);
        assert_eq!(a.selected().unwrap().ts, 3);
        a.query = "missing".into();
        a.filter();
        a.move_selection(-1);
        assert!(a.selected().is_none());
    }
}
