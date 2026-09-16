# Codex Prompt History

A Rust terminal UI for `~/.codex/history.jsonl` and session logs, built with Ratatui. Browse prompts, search text and session IDs, isolate a session, and see daily activity over the last 30 days. Open a prompt's session to explore its conversation and tool activity.

## Run

Install a current stable [Rust toolchain](https://rustup.rs), then:

```sh
cargo run --release
# Use a different file or start with a search:
cargo run --release -- --file /path/to/history.jsonl --query rust
# Explicitly combine multiple histories (the default still reads only ~/.codex):
cargo run --release -- \
  --file ~/.codex/history.jsonl \
  --file ~/.codex-beta/history.jsonl \
  --file ~/.codex-alpha/history.jsonl
# Install the executable:
cargo install --path .
codex-prompt-history
```

Requires an interactive terminal, at least 45 columns × 18 rows. Wide terminals show side-by-side panes; narrower terminals stack them. Dates use the local timezone.

## Release binaries (amd64 / x86-64)

Extract the archive for your operating system and run the executable from a terminal:

```sh
# Linux (static musl executable)
./codex-prompt-history
```

```powershell
# Windows (PowerShell or Windows Terminal)
.\codex-prompt-history.exe
# Optional custom history file
.\codex-prompt-history.exe --file C:\path\to\history.jsonl
```

The default history path is `$HOME/.codex/history.jsonl`, falling back to `%USERPROFILE%\.codex\history.jsonl` on Windows when `HOME` is unset.

To build both archives on Linux, install `musl-tools`, `gcc-mingw-w64-x86-64-posix`, Python 3, and Rust, then run:

```sh
rustup target add x86_64-unknown-linux-musl x86_64-pc-windows-gnu
bash scripts/release.sh
```

Archives and `SHA256SUMS` are written to `dist/`. Verify downloads on Linux with `sha256sum -c SHA256SUMS`; on Windows, compare `Get-FileHash <archive> -Algorithm SHA256` with the checksum file.

## Controls

| Key | Action |
| --- | --- |
| `Enter` | Open the selected prompt's session timeline |
| `Tab` / `Shift+Tab` | Switch focus between list and details |
| `a` | Focus / leave activity chart (prompt history) |
| `w` | Cycle 7-day / 30-day / all-time activity range |
| `d` | Clear date filter |
| `↑` / `↓`, `k` / `j` | Navigate the focused pane |
| `Home` / `End`, `g` / `G` | First / last item or detail line |
| `PgUp` / `PgDn` | Move ten list items or one page of details |
| `←` / `→`, `K` / `J` | Scroll the full prompt |
| `/` | Edit case-insensitive live search |
| `Enter` / `Esc` while searching | Finish editing, retaining search |
| `Ctrl+U` while searching | Clear query |
| `s` | Toggle filtering to the selected session |
| `t` | Cycle loaded sources: all, alpha, beta, gamma (when present) |
| `x` | Clear search |
| `[` / `]` | Previous / next failed tool in a session (wraps) |
| `o` | Toggle newest / oldest first |
| `r` | Reload the file |
| `Esc` | Leave activity focus; otherwise clear search, session, date, then source; return from a session when no filters remain |
| `?` | Show keyboard help |
| `q` / `Ctrl+C` | Quit |

The activity chart follows source, search, and session filters; the selected date filters the list while the chart keeps the surrounding days visible. Search matches literal substrings in prompt text or session IDs. Reload is manual; a failed reload preserves the current data. The app only reads history and never writes it or sends it anywhere.

Repeat `--file` to merge histories in timestamp order. Without `--file`, only `~/.codex/history.jsonl` is loaded; other directories are never discovered automatically. Multi-file views label each prompt's source. Session filtering distinguishes identical session IDs from different files, and `Enter` opens the source file's sibling `sessions` directory. `--sessions-dir` explicitly overrides that directory for all sources. Repeating the same file loads it once. Every requested file must be readable; reload (`r`) refreshes all sources together and preserves the current data if any file cannot be read.

## Activity dates

The chart defaults to the last 30 local-calendar days, including today. Press `a` to focus it; `←` / `→` moves one day, Page Up/Down moves seven days, and Home/End selects the range endpoints. The marker and bottom label show the exact date and matching prompt count, including zero-count days. `Enter` filters the prompt list to that local date and returns focus to the list. `d` clears the date filter. `Esc` leaves chart focus first.

`w` cycles 7 days, 30 days, and all time (earliest matching record through today). Changing the chart range alone does not restrict the list or clear an existing date filter. Source/search/session filters apply to both chart and list; the date filter applies only to the list so other days remain available for selection. Multi-file histories are counted together. Future dates are outside the chart ending today.

When the range contains more days than terminal columns, each column displays the **maximum daily count** in its interval. The header total and selected-day count remain exact; the chart label indicates this compressed mode. Sparse daily counts avoid allocating one entry for every date in a long history.

## Navigation and source profiles

The focused pane has a cyan border. `Tab` switches between the list and details; arrows, Home/End, and Page Up/Down operate on that pane. The filter strip shows active search, session, source, and matching record counts. `x` clears only search; `s` toggles session filtering; `t` cycles loaded sources and clears a source-specific session filter. `Esc` clears one filter at a time (search, session, date, source), then returns from a session. Editing search and dismissing help take precedence.

The following directory labels match these PowerShell profiles:

| Profile | History | Session directory |
| --- | --- | --- |
| alpha | `~/.codex/history.jsonl` | `~/.codex/sessions` |
| beta | `~/.codex-beta/history.jsonl` | `~/.codex-beta/sessions` |
| gamma | `~/.codex-gamma/history.jsonl` | `~/.codex-gamma/sessions` |

These are display labels, not new default sources. To load all three from PowerShell:

```powershell
.\codex-prompt-history.exe --file "$HOME\.codex\history.jsonl" --file "$HOME\.codex-beta\history.jsonl" --file "$HOME\.codex-gamma\history.jsonl"
```

Filtering and sorting retain the current record when it still matches. Reload locates the same record again even when new prompts have shifted its position. The app remains read-only and does not invoke the PowerShell functions or launch Codex sessions.

## Session history

Select a prompt and press `Enter` to load its session from `~/.codex/sessions/`. The timeline shows user messages, assistant replies, tool calls, and tool results in file order, with a full text preview. Session ID, working directory, and the last recorded model appear above the timeline.

JSON is typeset as indented fields and lists, including decoded multiline strings and nested JSON tool output. Each tool call and its result share one collapsed row, matched by `call_id` even when calls overlap. Groups stay at the call's position in the timeline. Select one and press `Enter` or `Space` to view input and result together. Search includes both. Reloading a session rebuilds pairs while preserving selection and expansion state when matching records still exist.

Tool rows show a compact command, file path, search query, or patch file when recognizable in the arguments. Long summaries are truncated to leave room for status; full input remains in the detail pane.

Tool rows show completion/failure and elapsed time when explicitly recorded in result metadata. Otherwise they show “Result received”. Calls without a matching result show “No result recorded”; unmatched and extra results remain separate so no records disappear.

Consecutive tool entries are folded into one **Tool activity** group by default, with an entry count and failure count. `Enter` / `Space` expands the group; select a tool inside it and use the same key to view its input and result. Press `c` to collapse all groups and tool details. Messages stay visible between groups. Search reveals matching individual tools regardless of group state; clearing search restores the group layout.

Press `]` / `[` in a session to jump to the next / previous failed tool. Navigation follows the current sort order and search filter, wraps at the ends, and expands both the containing group and the target tool. Explicit nonzero exit codes or error metadata identify failures; error words in ordinary output do not. The status line reports when no matching failures exist.

Results are labeled with their originating tool name. Expanded results prioritize readable output and error output, with explicit completion/failure indicators when recorded. Content wrappers are flattened; timing, other metadata, and call references appear below the output.

Use `/` to search session text, tool names, or roles; `↑` / `↓` to select an entry; `←` / `→` to scroll its content; `o` to reverse order; and `r` to reload the session. `Esc` first clears a session search; with no active filters it returns to prompt history, preserving its selection and filters. While editing search, `Esc` finishes editing first; `Ctrl+U` clears the query.

Session files are discovered recursively by metadata ID and loaded in the background only when opened. Missing logs show an error without closing the browser. With a custom history file, the default session directory is its sibling `sessions` folder. Override this for another directory or archived logs:

```sh
codex-prompt-history --sessions-dir /path/to/sessions
```

The viewer supports response-item messages and function/custom tool records, plus older event-only message logs. It avoids mirrored event-message duplicates. Non-text attachments appear as labels. System/developer instructions, reasoning records, and unrelated telemetry are omitted. Malformed JSON lines are skipped and counted. It displays the selected log only; fork ancestors are not reconstructed. Session entries are held in memory.

## Background loading and memory cache

Startup history loading, `r` reloads, session discovery, and session parsing run on a single background worker. The status line shows the current phase and processed line/file count; search, navigation, help, and quit remain available. `Esc` cancels a pending load before clearing filters or returning (finish editing search or close help first). A new request supersedes an older one, and cancelled or outdated results never replace the visible data. Cancellation is checked between records/files; it cannot interrupt an individual OS read or JSON parse already in progress. Failed reloads retain the current view.

Up to four parsed sessions are cached in memory with a 64 MiB estimated retained-data budget. Least recently used entries are evicted. Cache lookup includes the session file path, size, and modification time; session IDs are checked against their source directory. Reopening an unchanged cached session reports “Session loaded from memory cache”. `r` always bypasses the cache. Files that change while loading, exceed the budget, or lack modification timestamps are not cached. This budget covers retained cache entries; the active view, background result, and temporary parsing allocations are additional.

No history, session, or cache data is written to disk. The cache lasts only for this process. File metadata can miss externally modified content if its size and timestamp are deliberately preserved; use `r` to force a fresh read.

## Input format

One JSON object per line:

```json
{"session_id":"example-session","ts":1786406400,"text":"Explain this code"}
```

`ts` is Unix time in seconds. Additional fields are ignored. Blank lines are ignored; malformed records (including incomplete final lines) are skipped and counted in the header. Missing or unreadable files show a loading error in the TUI; press `r` to retry or `q` to quit. Records are held in memory.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```
