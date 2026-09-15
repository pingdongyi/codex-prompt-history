## Codex Prompt History v0.5.0

Browse multiple Codex history files together by repeating `--file`. The default remains `~/.codex/history.jsonl` only.

```sh
codex-prompt-history \
  --file ~/.codex/history.jsonl \
  --file ~/.codex-beta/history.jsonl \
  --file ~/.codex-alpha/history.jsonl
```

- Merge records in memory in timestamp order without writing a merged file or modifying source histories.
- Show each prompt's source and open sessions from that history file's sibling `sessions` directory.
- Keep identical session IDs from different sources separate when filtering.
- Load repeated file paths once and reload all requested sources together; failed reloads preserve current data.
- Preserve the existing explicit `--sessions-dir` override.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

20 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. A three-source interactive check verified session routing with identical IDs and unchanged source files. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
