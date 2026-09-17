use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Entry {
    #[serde(skip)]
    pub tool: Option<ToolInfo>,
    #[serde(skip)]
    pub source: Option<PathBuf>,
    pub session_id: String,
    pub ts: i64,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolInfo {
    pub summary: String,
    pub failed: bool,
    pub command: Option<RecordedCommand>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RecordedCommand {
    Shell(String),
    Arguments(Vec<String>),
}

#[derive(Clone, Default)]
pub struct History {
    pub sources: Vec<PathBuf>,
    pub entries: Vec<Entry>,
    pub skipped: usize,
}

impl History {
    fn load(path: &Path, progress: &mut crate::loader::Progress<'_>) -> Result<Self> {
        let file = File::open(path).with_context(|| format!("Cannot open {}", path.display()))?;
        let mut history = Self::parse_with(BufReader::new(file), progress)?;
        history.sources.push(path.to_path_buf());
        for entry in &mut history.entries {
            entry.source = Some(path.to_path_buf());
        }
        Ok(history)
    }

    #[cfg(test)]
    pub fn load_many(paths: &[PathBuf]) -> Result<Self> {
        Self::load_many_with(paths, &mut |_, _| Ok(()))
    }

    pub fn load_many_with(
        paths: &[PathBuf],
        progress: &mut crate::loader::Progress<'_>,
    ) -> Result<Self> {
        let mut merged = Self::default();
        let mut seen = std::collections::HashSet::new();
        for path in paths {
            progress("Opening history files", merged.entries.len())?;
            let canonical = path
                .canonicalize()
                .with_context(|| format!("Cannot open {}", path.display()))?;
            if !seen.insert(canonical.clone()) {
                continue;
            }
            let history = Self::load(&canonical, progress)?;
            merged.sources.extend(history.sources);
            merged.entries.extend(history.entries);
            merged.skipped += history.skipped;
        }
        progress("Sorting history", merged.entries.len())?;
        merged
            .entries
            .sort_by_key(|entry| std::cmp::Reverse(entry.ts));
        Ok(merged)
    }

    #[cfg(test)]
    fn parse(reader: impl BufRead) -> Result<Self> {
        Self::parse_with(reader, &mut |_, _| Ok(()))
    }

    fn parse_with(
        reader: impl BufRead,
        progress: &mut crate::loader::Progress<'_>,
    ) -> Result<Self> {
        let mut history = Self::default();
        for (index, line) in reader.lines().enumerate() {
            progress("Reading history lines", index + 1)?;
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

/// Display the parent directory name without profile aliases.
pub fn source_name(path: &Path) -> String {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    parent
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| parent.display().to_string())
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
    fn source_names_are_parent_directory_names() {
        assert_eq!(source_name(Path::new(".codex/history.jsonl")), ".codex");
        assert_eq!(
            source_name(Path::new(".codex-beta/history.jsonl")),
            ".codex-beta"
        );
        assert_eq!(
            source_name(Path::new(".codex-gamma/history.jsonl")),
            ".codex-gamma"
        );
        assert_eq!(
            source_name(Path::new(".codex-alpha/history.jsonl")),
            ".codex-alpha"
        );
    }
    #[test]
    fn merges_files_without_duplicate_sources_and_preserves_origins() {
        let root = std::env::temp_dir().join(format!(
            "codex-history-sources-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let a = root.join("a.jsonl");
        let b = root.join("b.jsonl");
        std::fs::write(
            &a,
            "{\"session_id\":\"same\",\"ts\":1,\"text\":\"one\"}\nbad\n",
        )
        .unwrap();
        std::fs::write(&b, "{\"session_id\":\"same\",\"ts\":2,\"text\":\"two\"}\n").unwrap();
        let empty = root.join("empty.jsonl");
        std::fs::write(&empty, "").unwrap();
        let history = History::load_many(&[a.clone(), b.clone(), empty, a.clone()]).unwrap();
        assert_eq!(history.sources.len(), 3);
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.skipped, 1);
        assert_eq!(history.entries[0].text, "two");
        assert_eq!(
            history.entries[0].source.as_ref(),
            Some(&b.canonicalize().unwrap())
        );
        assert_eq!(
            history.entries[1].source.as_ref(),
            Some(&a.canonicalize().unwrap())
        );
        std::fs::write(
            &b,
            "{\"session_id\":\"same\",\"ts\":3,\"text\":\"updated\"}\n",
        )
        .unwrap();
        assert_eq!(
            History::load_many(&[a.clone(), b.clone()]).unwrap().entries[0].text,
            "updated"
        );
        assert!(History::load_many(&[a, root.join("missing")]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
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
