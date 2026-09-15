## Codex Prompt History v0.5.1

Toggling the session filter with `s` now keeps the currently selected prompt instead of jumping to the first entry.

- Preserve selection when enabling and disabling the session filter.
- Retain the prompt preview's scroll position and the selection's relative list position where possible.
- Preserve the current selection after navigating within a filtered session.
- Add regression coverage for filter toggling with search and reversed ordering.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

21 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
