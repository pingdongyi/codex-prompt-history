## Codex Prompt History v0.4.7

Search input now uses the terminal's default foreground color for readability across light and dark themes.

- Keep typed text the same color while editing and after leaving the search field.
- Render placeholder text in gray.
- Highlight the border, rather than the text, while editing search.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

17 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
