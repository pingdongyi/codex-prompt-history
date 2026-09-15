## Codex Prompt History v0.4.6

Tool activity groups now use tighter internal spacing while preserving the spacing around the outer conversation list.

- Remove blank rows between tool entries within expanded groups.
- Preserve a single gap between groups and surrounding messages.
- Keep selection highlighting aligned with content without erasing compact tool rows.
- Add a rendering regression test for nested spacing and selected content visibility.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

17 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
