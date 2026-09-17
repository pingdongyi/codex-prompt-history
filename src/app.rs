use crate::history::{Entry, History};
use ratatui::widgets::ListState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    List,
    Detail,
    Activity,
}

struct RecordMark {
    source: Option<std::path::PathBuf>,
    session_id: String,
    ts: i64,
    digest: u64,
}
impl RecordMark {
    fn digest(entry: &Entry) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        entry.text.hash(&mut hash);
        hash.finish()
    }
    fn new(entry: &Entry) -> Self {
        Self {
            source: entry.source.clone(),
            session_id: entry.session_id.clone(),
            ts: entry.ts,
            digest: Self::digest(entry),
        }
    }
    fn matches(&self, entry: &Entry) -> bool {
        self.source == entry.source
            && self.session_id == entry.session_id
            && self.ts == entry.ts
            && self.digest == Self::digest(entry)
    }
}
struct SearchBookmark {
    query: String,
    mark: Option<RecordMark>,
    occurrence: usize,
    group: bool,
    row: usize,
    scroll: usize,
    focus: Focus,
    activity_cursor: chrono::NaiveDate,
}

pub struct App {
    pub history: History,
    pub visible: Vec<usize>,
    pub list: ListState,
    pub query: String,
    pub searching: bool,
    pub find: crate::detail_find::Find,
    pub detail_revision: u64,
    pub search: crate::search::Editor,
    search_bookmark: Option<SearchBookmark>,
    pub session: Option<String>,
    pub session_origin: Option<std::path::PathBuf>,
    pub source_filter: Option<std::path::PathBuf>,
    pub focus: Focus,
    pub detail_page_size: usize,
    pub detail_max_scroll: usize,
    pub matched_count: usize,
    pub activity: crate::activity::Activity,
    pub activity_window: crate::activity::Window,
    pub activity_cursor: chrono::NaiveDate,
    pub date_filter: Option<chrono::NaiveDate>,
    selection_anchor: Option<(usize, usize)>,
    pub oldest_first: bool,
    pub scroll: usize,
    pub help: bool,
    pub help_scroll: usize,
    pub help_max_scroll: usize,
    pub help_page: usize,
    pub status: String,
    pub loading: bool,
    pub transcript: Option<Box<App>>,
    pub session_source: Option<std::path::PathBuf>,
    pub session_info: String,
    pub session_id: Option<String>,
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
            find: crate::detail_find::Find::default(),
            detail_revision: 0,
            search: crate::search::Editor::default(),
            search_bookmark: None,
            session: None,
            session_origin: None,
            source_filter: None,
            focus: Focus::List,
            detail_page_size: 1,
            detail_max_scroll: 0,
            matched_count: 0,
            activity: crate::activity::Activity::default(),
            activity_window: crate::activity::Window::default(),
            activity_cursor: chrono::Local::now().date_naive(),
            date_filter: None,
            selection_anchor: None,
            oldest_first: false,
            scroll: 0,
            help: false,
            help_scroll: 0,
            help_max_scroll: 0,
            help_page: 1,
            status: String::new(),
            loading: false,
            transcript: None,
            session_source: None,
            session_info: String::new(),
            session_id: None,
            expanded_tools: std::collections::HashSet::new(),
            expanded_groups: std::collections::HashSet::new(),
            groups: std::collections::BTreeMap::new(),
        };
        app.filter();
        app
    }

    pub fn is_editing(&self) -> bool {
        self.searching || self.find.editing
    }

    pub fn begin_find(&mut self) {
        if self.selected().is_none() {
            self.status = "Select an individual record to search its details".into();
            return;
        }
        if let Some(&index) = self
            .list
            .selected()
            .and_then(|position| self.visible.get(position))
            && is_tool(&self.history.entries[index])
        {
            self.expanded_tools.insert(index);
        }
        self.find.editor.history = self.search.history.clone();
        self.find.begin(self.scroll, self.focus);
        self.focus = Focus::Detail;
    }

    pub fn jump_detail_match(&mut self, forward: bool) {
        if self.selected().is_none() || self.find.query.is_empty() {
            self.status = "Press f on a record to search its details".into();
            return;
        }
        if let Some(&index) = self
            .list
            .selected()
            .and_then(|position| self.visible.get(position))
            && is_tool(&self.history.entries[index])
        {
            self.expanded_tools.insert(index);
        }
        self.find.next(forward);
        self.focus = Focus::Detail;
    }

    pub fn begin_search(&mut self) {
        if self.is_editing() {
            return;
        }
        let selected = self
            .list
            .selected()
            .and_then(|position| self.visible.get(position))
            .copied();
        let group = selected.and_then(|index| self.groups.get(&index));
        let index = group.map(|range| range.start).or(selected);
        let mark = index
            .and_then(|index| self.history.entries.get(index))
            .map(RecordMark::new);
        let occurrence = mark.as_ref().zip(index).map_or(0, |(mark, index)| {
            self.history.entries[..index]
                .iter()
                .filter(|entry| mark.matches(entry))
                .count()
        });
        self.search_bookmark = Some(SearchBookmark {
            query: self.query.clone(),
            mark,
            occurrence,
            group: group.is_some(),
            row: self
                .list
                .selected()
                .unwrap_or(0)
                .saturating_sub(self.list.offset()),
            scroll: self.scroll,
            focus: self.focus,
            activity_cursor: self.activity_cursor,
        });
        self.search.begin(&self.query);
        self.searching = true;
    }

    pub fn confirm_search(&mut self) {
        self.search.remember(&self.query);
        self.search_bookmark = None;
        self.searching = false;
    }

    pub fn cancel_search(&mut self) {
        self.searching = false;
        if let Some(bookmark) = self.search_bookmark.take() {
            self.query = bookmark.query;
            self.filter();
            if let Some(index) = bookmark.mark.as_ref().and_then(|mark| {
                self.history
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| mark.matches(entry))
                    .nth(bookmark.occurrence)
                    .map(|(index, _)| index)
            }) {
                let target = if bookmark.group {
                    self.group_for_entry(index).map_or(index, |(&id, _)| id)
                } else {
                    index
                };
                if let Some(position) = self.visible_position(target) {
                    self.list.select(Some(position));
                    *self.list.offset_mut() = position.saturating_sub(bookmark.row);
                    self.scroll = bookmark.scroll;
                    self.find.preserve_scroll();
                }
            }
            self.focus = bookmark.focus;
            let (start, end) = self
                .activity
                .bounds(self.activity_window, chrono::Local::now().date_naive());
            self.activity_cursor = bookmark.activity_cursor.clamp(start, end);
        }
        self.search.begin(&self.query);
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
        app.session_id = (!session.id.is_empty()).then_some(session.id);
        app.session_info = session.info;
        app.filter();
        app
    }

    pub fn group_for_entry(&self, index: usize) -> Option<(&usize, &std::ops::Range<usize>)> {
        if index >= self.history.entries.len() {
            return None;
        }
        self.groups
            .range(..=self.history.entries.len() + index)
            .next_back()
            .filter(|(_, range)| range.contains(&index))
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
        let target = selected.map(|index| self.group_for_entry(index).map_or(index, |(&id, _)| id));
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

    pub fn jump_failure(&mut self, forward: bool) {
        if self.session_source.is_none() {
            return;
        }
        let query = self.query.to_lowercase();
        let mut failures: Vec<usize> = self
            .history
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| is_failed_tool(entry) && matches_query(entry, &query))
            .map(|(index, _)| index)
            .collect();
        if self.oldest_first {
            failures.reverse();
        }
        if failures.is_empty() {
            self.status = "No failed tools match the current filters".into();
            return;
        }
        let selected = self
            .list
            .selected()
            .and_then(|position| self.visible.get(position))
            .copied();
        let group = selected.and_then(|id| self.groups.get(&id));
        let current = group
            .map(|range| {
                if self.oldest_first {
                    range.end - 1
                } else {
                    range.start
                }
            })
            .or(selected);
        let order = |index: usize| {
            if self.oldest_first {
                self.history.entries.len() - 1 - index
            } else {
                index
            }
        };
        let target = if forward {
            failures
                .iter()
                .copied()
                .find(|&index| {
                    current.is_none_or(|current| {
                        order(index) > order(current) || group.is_some() && index == current
                    })
                })
                .unwrap_or(failures[0])
        } else {
            failures
                .iter()
                .rev()
                .copied()
                .find(|&index| current.is_none_or(|current| order(index) < order(current)))
                .unwrap_or(*failures.last().unwrap())
        };
        if let Some((&id, _)) = self.group_for_entry(target) {
            self.expanded_groups.insert(id);
        }
        self.expanded_tools.insert(target);
        self.filter();
        self.list
            .select(self.visible.iter().position(|&index| index == target));
        self.scroll = 0;
        self.focus = Focus::List;
        let number = failures.iter().position(|&index| index == target).unwrap() + 1;
        self.status = format!(
            "Failed tool {number}/{} · [ previous · ] next (wraps)",
            failures.len()
        );
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
        let matches: Vec<usize> = self
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
                    && matches_query(entry, &query)
            })
            .map(|(i, _)| i)
            .collect();
        self.activity.counts.clear();
        if self.session_source.is_none() {
            for &index in &matches {
                if let Some(date) = crate::activity::local_date(self.history.entries[index].ts) {
                    *self.activity.counts.entry(date).or_default() += 1;
                }
            }
        }
        self.visible = matches
            .into_iter()
            .filter(|&index| {
                self.date_filter.is_none_or(|date| {
                    crate::activity::local_date(self.history.entries[index].ts) == Some(date)
                })
            })
            .collect();
        let (start, end) = self
            .activity
            .bounds(self.activity_window, chrono::Local::now().date_naive());
        self.activity_cursor = self.activity_cursor.clamp(start, end);
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
                    if let Some((&id, _)) = self.group_for_entry(index) {
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
            self.group_for_entry(index)
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
        self.detail_revision = self.detail_revision.wrapping_add(1);
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
            self.group_for_entry(index)
                .and_then(|(&id, _)| self.visible.iter().position(|&i| i == id))
                .or_else(|| self.visible_position(index))
        } else {
            self.visible_position(index)
        } {
            self.list.select(Some(position));
            *self.list.offset_mut() = position.saturating_sub(row);
            self.scroll = scroll;
            self.find.preserve_scroll();
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
        match self.focus {
            Focus::List => self.move_selection(delta),
            Focus::Detail => {
                self.scroll = self
                    .scroll
                    .saturating_add_signed(delta)
                    .min(self.detail_max_scroll)
            }
            Focus::Activity => {
                let (start, end) = self
                    .activity
                    .bounds(self.activity_window, chrono::Local::now().date_naive());
                let offset = self
                    .activity_cursor
                    .clamp(start, end)
                    .signed_duration_since(start)
                    .num_days() as usize;
                let max = end.signed_duration_since(start).num_days() as usize;
                self.activity_cursor = start
                    .checked_add_days(chrono::Days::new(
                        offset.saturating_add_signed(delta).min(max) as u64,
                    ))
                    .unwrap_or(end);
            }
        }
    }

    pub fn page(&mut self, direction: isize) {
        let size = match self.focus {
            Focus::Detail => self.detail_page_size,
            Focus::Activity => 7,
            Focus::List => 10,
        };
        self.navigate(direction * size.max(1) as isize);
    }

    pub fn focus_activity(&mut self) {
        if self.session_source.is_none() {
            self.focus = if self.focus == Focus::Activity {
                Focus::List
            } else {
                Focus::Activity
            };
        }
    }

    pub fn cycle_activity_window(&mut self) {
        self.activity_window = self.activity_window.next();
        let (start, end) = self
            .activity
            .bounds(self.activity_window, chrono::Local::now().date_naive());
        self.activity_cursor = self.activity_cursor.clamp(start, end);
    }

    pub fn apply_activity_date(&mut self) {
        self.date_filter = Some(self.activity_cursor);
        self.filter();
        self.focus = Focus::List;
    }

    pub fn clear_one_filter(&mut self) -> bool {
        if !self.query.is_empty() {
            self.query.clear();
        } else if self.session.is_some() {
            self.session = None;
            self.session_origin = None;
        } else if self.date_filter.is_some() {
            self.date_filter = None;
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
        if let Some(date) = self.date_filter {
            parts.push(format!("Date: {date} [d]"));
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

fn matches_query(entry: &Entry, query: &str) -> bool {
    query.is_empty()
        || entry.text.to_lowercase().contains(query)
        || entry.session_id.to_lowercase().contains(query)
        || entry
            .tool
            .as_ref()
            .is_some_and(|tool| tool.summary.to_lowercase().contains(query))
}

pub fn is_tool(entry: &Entry) -> bool {
    entry.session_id.starts_with("TOOL ·") || entry.session_id.starts_with("RESULT ·")
}

pub fn is_failed_tool(entry: &Entry) -> bool {
    is_tool(entry)
        && entry.tool.as_ref().map_or_else(
            || entry.text.starts_with("Failed") || entry.text.starts_with("FAILED"),
            |tool| tool.failed,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_cancel_restores_query_selection_and_scroll_after_reload() {
        let mut a = app();
        a.move_selection(2);
        a.scroll = 5;
        a.focus = Focus::Detail;
        a.begin_search();
        a.search.insert(&mut a.query, "no matching records");
        a.filter();
        assert!(a.visible.is_empty());
        let mut updated = app().history;
        updated.entries.insert(
            0,
            Entry {
                tool: None,
                source: None,
                session_id: "new".into(),
                ts: 4,
                text: "new prompt".into(),
            },
        );
        a.replace_history(updated);
        a.cancel_search();
        assert!(a.query.is_empty());
        assert!(!a.searching);
        assert_eq!(a.selected().unwrap().ts, 1);
        assert_eq!(a.scroll, 5);
        assert_eq!(a.focus, Focus::Detail);
        assert!(a.search.history.borrow().is_empty());
        a.begin_search();
        a.search.insert(&mut a.query, "RUST");
        a.filter();
        a.confirm_search();
        assert_eq!(a.search.history.borrow().as_slice(), &["RUST"]);
        assert_eq!(a.visible.len(), 2);
    }
    #[test]
    fn date_filter_uses_local_days_without_hiding_the_chart_context() {
        let today = chrono::Local::now().date_naive();
        let yesterday = today.pred_opt().unwrap();
        let ts = |date: chrono::NaiveDate| {
            date.and_hms_opt(12, 0, 0)
                .unwrap()
                .and_local_timezone(chrono::Local)
                .earliest()
                .unwrap()
                .timestamp()
        };
        let alpha = std::path::PathBuf::from(".codex/history.jsonl");
        let beta = std::path::PathBuf::from(".codex-beta/history.jsonl");
        let mut a = App::new(History {
            entries: vec![
                Entry {
                    tool: None,
                    source: Some(alpha.clone()),
                    session_id: "same".into(),
                    ts: ts(today),
                    text: "today alpha".into(),
                },
                Entry {
                    tool: None,
                    source: Some(beta.clone()),
                    session_id: "same".into(),
                    ts: ts(today),
                    text: "today beta".into(),
                },
                Entry {
                    tool: None,
                    source: Some(alpha),
                    session_id: "same".into(),
                    ts: ts(yesterday),
                    text: "yesterday".into(),
                },
            ],
            skipped: 0,
        });
        a.focus_activity();
        a.navigate(-1);
        assert_eq!(a.activity_cursor, yesterday);
        assert_eq!(a.activity.count(yesterday), 1);
        a.apply_activity_date();
        assert_eq!(a.focus, Focus::List);
        assert_eq!(a.visible, vec![2]);
        assert_eq!(a.activity.count(today), 2);
        assert!(a.filter_summary().contains(&format!("Date: {yesterday}")));
        a.source_filter = Some(beta);
        a.filter();
        assert!(a.visible.is_empty());
        assert_eq!(a.activity.count(today), 1);
        assert!(a.clear_one_filter());
        assert_eq!(a.date_filter, None);
        assert_eq!(a.visible, vec![1]);
        a.focus_activity();
        a.navigate(isize::MAX);
        assert_eq!(a.activity_cursor, today);
        a.navigate(isize::MIN);
        assert_eq!(
            a.activity_cursor,
            today.checked_sub_days(chrono::Days::new(29)).unwrap()
        );
        a.apply_activity_date();
        assert!(a.visible.is_empty());
    }
    #[test]
    fn failure_navigation_expands_groups_wraps_and_respects_search_and_order() {
        let mut a = app();
        a.session_source = Some("session".into());
        a.history.entries = vec![
            Entry {
                tool: None,
                source: None,
                session_id: "USER".into(),
                ts: 0,
                text: "Failed is just a word here".into(),
            },
            Entry {
                tool: Some(crate::history::ToolInfo {
                    command: None,
                    summary: "one".into(),
                    failed: true,
                }),
                source: None,
                session_id: "TOOL · shell".into(),
                ts: 1,
                text: "Failed\nneedle one".into(),
            },
            Entry {
                tool: Some(crate::history::ToolInfo {
                    command: None,
                    summary: "ok".into(),
                    failed: false,
                }),
                source: None,
                session_id: "TOOL · shell".into(),
                ts: 2,
                text: "Completed".into(),
            },
            Entry {
                tool: Some(crate::history::ToolInfo {
                    command: None,
                    summary: "two".into(),
                    failed: true,
                }),
                source: None,
                session_id: "TOOL · read".into(),
                ts: 3,
                text: "Failed\nneedle two".into(),
            },
        ];
        a.filter();
        assert_eq!(a.visible, vec![0, 5]);
        a.jump_failure(true);
        assert_eq!(a.selected().unwrap().ts, 1);
        assert!(a.expanded_groups.contains(&5));
        assert!(!a.tool_collapsed(1));
        a.jump_failure(false);
        assert_eq!(a.selected().unwrap().ts, 3);
        a.oldest_first = true;
        a.filter();
        a.jump_failure(true);
        assert_eq!(a.selected().unwrap().ts, 1);
        a.query = "two".into();
        a.filter();
        a.jump_failure(true);
        assert_eq!(a.selected().unwrap().ts, 3);
        a.history.entries[3].tool.as_mut().unwrap().summary = "summary-only match".into();
        a.query = "summary-only".into();
        a.filter();
        a.jump_failure(true);
        assert_eq!(a.selected().unwrap().ts, 3);
        a.query = "Completed".into();
        a.filter();
        let selected = a.list.selected();
        a.jump_failure(true);
        assert_eq!(a.list.selected(), selected);
        assert!(a.status.contains("No failed tools"));
    }
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
                tool: None,
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
                    tool: None,
                    source: None,
                    session_id: "USER".into(),
                    ts: 1,
                    text: "hello".into(),
                },
                Entry {
                    tool: None,
                    source: None,
                    session_id: "TOOL · shell".into(),
                    ts: 2,
                    text: "Completed\nneedle".into(),
                },
                Entry {
                    tool: None,
                    source: None,
                    session_id: "TOOL · search".into(),
                    ts: 3,
                    text: "Failed\noutput".into(),
                },
                Entry {
                    tool: None,
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
                    tool: None,
                    source: None,
                    session_id: "a".into(),
                    ts: 3,
                    text: "RUST 世界".into(),
                },
                Entry {
                    tool: None,
                    source: None,
                    session_id: "b".into(),
                    ts: 2,
                    text: "other".into(),
                },
                Entry {
                    tool: None,
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
