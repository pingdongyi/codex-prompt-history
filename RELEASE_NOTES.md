## Codex Prompt History v0.15.0

### Reading

- Press `v` for fullscreen details and `Esc` to return to the previous pane.
- Press `W` to toggle automatic wrapping; with wrapping off and details focused, left/right arrows pan horizontally.
- Keep reading position when reflowing text, and reveal detail-search matches when jumping to them.

### Improvements since v0.5.2

- Source-aware navigation and filtering, with source names taken from each history file’s parent directory.
- Tool command/file summaries, paired results, compact folded activity groups, and previous/next failed-tool navigation.
- Activity date selection and filtering, plus 7-day, 30-day, and all-time chart windows.
- Copy record content, session IDs, and complete tool commands using native clipboard or terminal OSC 52.
- Unicode-aware search editing, recent query history, scrollable shortcut help, and detail search with highlighting and match navigation.
- Background loading, cancellation, and bounded in-memory session caching.
- Resume a selected session in its recorded project directory using its source-specific CODEX_HOME; explicit project and CLI overrides are available.
- Sort by time, session activity, or session size; changing sort selects the first record and resets scrolling.
- Source picker, filter reset, and statistics for matching records, sessions, sources, activity, and tool failures.

Default loading remains only `~/.codex/history.jsonl`. Repeat `--file` to merge sources in memory; each source uses its own sessions directory. Browsing never rewrites history or session files.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

55 Rust tests, Clippy, formatting, and Linux terminal smoke tests pass. Resume tests use a fake CLI and do not launch real sessions. Both targets build successfully; the Windows binary is cross-compiled and has not been runtime-tested on Windows.
