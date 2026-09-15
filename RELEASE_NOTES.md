## Codex Prompt History v0.4.5

A Rust terminal browser for Codex prompt history and session conversations.

- Search prompts and session messages, filter sessions, and visualize 30-day activity.
- Pair tool calls with their results and collapse consecutive tools into expandable groups.
- Read structured tool output, completion status, and error details.
- Use compact list spacing with content-aligned selection highlighting.

### Downloads

- Linux amd64: static musl executable in the `.tar.gz` archive.
- Windows amd64: executable in the `.zip` archive.
- `SHA256SUMS`: checksums for both archives.

Extract the archive and run the executable in a terminal. Press `?` for keyboard shortcuts.

### Validation

16 Rust tests, Clippy, formatting, and Linux interactive smoke checks pass. The Windows binary is cross-compiled; Windows runtime testing has not been performed.
