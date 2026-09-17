use crate::history::Entry;
use chrono::{Days, Local, NaiveDate};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Sort {
    #[default]
    Time,
    SessionRecent,
    SessionCount,
}

pub fn sort(entries: &[Entry], indices: &mut [usize], mode: Sort) {
    if mode == Sort::Time {
        indices.sort_by_key(|&index| (std::cmp::Reverse(entries[index].ts), index));
        return;
    }
    let mut sessions = HashMap::new();
    for &index in indices.iter() {
        let entry = &entries[index];
        let metric = sessions
            .entry((entry.source.as_ref(), entry.session_id.as_str()))
            .or_insert((0usize, entry.ts));
        metric.0 += 1;
        metric.1 = metric.1.max(entry.ts);
    }
    indices.sort_by(|&a, &b| {
        let left = &entries[a];
        let right = &entries[b];
        let left_key = (left.source.as_ref(), left.session_id.as_str());
        let right_key = (right.source.as_ref(), right.session_id.as_str());
        let group = if mode == Sort::Time || left_key == right_key {
            std::cmp::Ordering::Equal
        } else {
            let l = sessions[&left_key];
            let r = sessions[&right_key];
            match mode {
                Sort::SessionCount => r.0.cmp(&l.0).then_with(|| r.1.cmp(&l.1)),
                _ => r.1.cmp(&l.1),
            }
            .then_with(|| left_key.cmp(&right_key))
        };
        group
            .then_with(|| right.ts.cmp(&left.ts))
            .then_with(|| a.cmp(&b))
    });
}

#[derive(Default)]
pub struct Stats {
    pub records: usize,
    pub sessions: usize,
    pub sources: BTreeMap<PathBuf, usize>,
    pub days: BTreeMap<NaiveDate, usize>,
    pub first: Option<i64>,
    pub last: Option<i64>,
    pub users: usize,
    pub assistants: usize,
    pub tools: usize,
    pub results: usize,
    pub failures: usize,
    pub busiest: Option<(Option<PathBuf>, String, usize)>,
}
impl Stats {
    pub fn build(entries: &[Entry], indices: impl IntoIterator<Item = usize>) -> Self {
        let mut stats = Self::default();
        let mut sessions = HashMap::new();
        for index in indices {
            let entry = &entries[index];
            stats.records += 1;
            let metric = sessions
                .entry((entry.source.as_ref(), entry.session_id.as_str()))
                .or_insert((0usize, entry.ts));
            metric.0 += 1;
            metric.1 = metric.1.max(entry.ts);
            if let Some(source) = &entry.source {
                if let Some(count) = stats.sources.get_mut(source) {
                    *count += 1;
                } else {
                    stats.sources.insert(source.clone(), 1);
                }
            }
            if let Some(date) = crate::activity::local_date(entry.ts) {
                *stats.days.entry(date).or_default() += 1;
            }
            stats.first = Some(stats.first.map_or(entry.ts, |first| first.min(entry.ts)));
            stats.last = Some(stats.last.map_or(entry.ts, |last| last.max(entry.ts)));
            match entry.session_id.as_str() {
                "USER" => stats.users += 1,
                "ASSISTANT" => stats.assistants += 1,
                name if name.starts_with("TOOL ·") => stats.tools += 1,
                name if name.starts_with("RESULT ·") => stats.results += 1,
                _ => {}
            }
            if crate::app::is_failed_tool(entry) {
                stats.failures += 1;
            }
        }
        stats.sessions = sessions.len();
        stats.busiest = sessions
            .into_iter()
            .max_by(|(left_key, left), (right_key, right)| {
                left.cmp(right).then_with(|| right_key.cmp(left_key))
            })
            .map(|((source, id), (count, _))| (source.cloned(), id.to_owned(), count));
        stats
    }
    pub fn recent(&self, days: u64) -> usize {
        let today = Local::now().date_naive();
        let start = today
            .checked_sub_days(Days::new(days.saturating_sub(1)))
            .unwrap_or(today);
        self.days.range(start..=today).map(|(_, count)| count).sum()
    }
    pub fn peak(&self) -> Option<(NaiveDate, usize)> {
        self.days
            .iter()
            .max_by_key(|(date, count)| (*count, *date))
            .map(|(&date, &count)| (date, count))
    }
}

pub enum PanelKind {
    Order,
    Sources(Vec<(Option<PathBuf>, usize)>),
    Stats,
}
pub struct Panel {
    pub kind: PanelKind,
    pub selected: usize,
    pub scroll: usize,
    pub max_scroll: usize,
    pub page: usize,
}
impl Panel {
    pub fn new(kind: PanelKind, selected: usize) -> Self {
        Self {
            kind,
            selected,
            scroll: 0,
            max_scroll: 0,
            page: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_sorting_and_statistics_keep_source_identity() {
        let entry = |source: &str, id: &str, ts| Entry {
            source: Some(PathBuf::from(source)),
            tool: None,
            session_id: id.into(),
            ts,
            text: "prompt".into(),
        };
        let entries = vec![
            entry("a/history", "same", 4),
            entry("b/history", "same", 8),
            entry("a/history", "other", 9),
            entry("a/history", "same", 2),
            entry("a/history", "same", 1),
            entry("b/history", "same", 0),
        ];
        let mut indices: Vec<_> = (0..entries.len()).collect();
        sort(&entries, &mut indices, Sort::Time);
        assert_eq!(indices, vec![2, 1, 0, 3, 4, 5]);
        sort(&entries, &mut indices, Sort::SessionRecent);
        assert_eq!(indices, vec![2, 1, 5, 0, 3, 4]);
        sort(&entries, &mut indices, Sort::SessionCount);
        assert_eq!(indices, vec![0, 3, 4, 1, 5, 2]);
        let stats = Stats::build(&entries, 0..entries.len());
        assert_eq!(stats.records, 6);
        assert_eq!(stats.sessions, 3);
        assert_eq!(stats.sources.len(), 2);
        assert_eq!(stats.first, Some(0));
        assert_eq!(stats.last, Some(9));
        assert_eq!(
            stats.busiest,
            Some((Some(PathBuf::from("a/history")), "same".into(), 3))
        );
        let filtered = Stats::build(&entries, [1, 5]);
        assert_eq!(filtered.sessions, 1);
        assert_eq!(filtered.records, 2);
    }
}
