## Codex Prompt History v0.5.2

The 30-day activity chart now fills the available panel width and adapts when the terminal is resized.

- Spread daily bars across the full chart width, with today at the right edge.
- Preserve the original daily counts, matching-prompt total, and vertical scale.
- Continue aggregating all loaded histories and applying current search/session filters.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

21 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
