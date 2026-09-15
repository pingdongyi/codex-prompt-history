use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};

#[derive(Debug, Deserialize)]
pub struct Entry {
    pub session_id: String,
    pub ts: i64,
    pub text: String,
}

#[derive(Default)]
pub struct History {
    pub entries: Vec<Entry>,
    pub skipped: usize,
}

impl History {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
        Self::parse(BufReader::new(file))
    }

    fn parse(reader: impl BufRead) -> Result<Self> {
        let mut history = Self::default();
        for line in reader.lines() {
            let line = line.context("Cannot read history")?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<Entry>(&line) {
                Ok(entry) => history.entries.push(entry),
                Err(_) => history.skipped += 1,
            }
        }
        history
            .entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.ts));
        Ok(history)
    }
}

pub fn timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| {
            dt.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|| format!("timestamp {ts}"))
}

/// Remove terminal control characters while preserving prompt layout.
pub fn display_text(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .collect::<String>()
        .replace('\t', "    ")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_unicode_skips_bad_records_and_sorts() {
        let input = "{\"session_id\":\"a\",\"ts\":1,\"text\":\"你好\\nworld\"}\ninvalid\n\n{\"session_id\":\"b\",\"ts\":3,\"text\":\"new\",\"extra\":true}\n{\"ts\":4}";
        let history = History::parse(input.as_bytes()).unwrap();
        assert_eq!(history.skipped, 2);
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0].ts, 3);
        assert_eq!(history.entries[1].text, "你好\nworld");
    }
    #[test]
    fn empty_history_is_valid() {
        assert!(History::parse("\n".as_bytes()).unwrap().entries.is_empty());
    }
    #[test]
    fn strips_control_characters() {
        assert_eq!(display_text("a\u{1b}\u{7}\n\tb"), "a\n    b");
    }
}
