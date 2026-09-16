use crate::{
    app::{App, Focus},
    history::{display_text, timestamp},
};
use chrono::{Days, Local};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph, Sparkline, Wrap},
};
use std::collections::HashSet;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;

fn panel(title: impl Into<String>) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(MUTED))
        .title(Span::styled(title.into(), Style::default().fg(ACCENT)))
}

/// Outer items share one blank row; children inside a tool group stay compact.
fn spaced_item(mut lines: Vec<Line<'static>>, spacer: bool) -> ListItem<'static> {
    while lines.last().is_some_and(|line| line.width() == 0) {
        lines.pop();
    }
    if spacer {
        lines.push(Line::from(""));
    }
    ListItem::new(lines)
}

fn elide(text: &str, width: usize) -> String {
    let text = display_text(text).replace('\n', " ");
    if Span::raw(&text).width() <= width {
        return text;
    }
    if width == 0 {
        return String::new();
    }
    let mut used = 0;
    let mut result = String::new();
    for c in text.chars() {
        let size = Span::raw(c.to_string()).width();
        if used + size > width - 1 {
            break;
        }
        result.push(c);
        used += size;
    }
    result.push('…');
    result
}

fn tool_label(entry: &crate::history::Entry, width: usize) -> String {
    let summary = entry.text.lines().next().filter(|line| {
        line.starts_with("Completed")
            || line.starts_with("Failed")
            || line.starts_with("Result received")
            || *line == "No result recorded"
    });
    let status = if crate::app::is_failed_tool(entry) {
        summary
            .filter(|s| s.starts_with("Failed"))
            .unwrap_or("Failed")
            .to_owned()
    } else {
        summary
            .map(str::to_owned)
            .unwrap_or_else(|| timestamp(entry.ts))
    };
    let status = elide(&status, (width / 2).max(1));
    let name = elide(
        entry
            .session_id
            .strip_prefix("TOOL · ")
            .unwrap_or(&entry.session_id),
        (width / 3).clamp(1, 24),
    );
    let available = width.saturating_sub(Span::raw(&name).width() + Span::raw(&status).width() + 8);
    if let Some(action) = entry
        .tool
        .as_ref()
        .map(|tool| &tool.summary)
        .filter(|s| !s.is_empty())
        && available >= 4
    {
        format!("▸ {name} · {} · {status}", elide(action, available))
    } else {
        format!("▸ {name} · {status}")
    }
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let transcript = app.session_source.is_some();
    if area.width < 45 || area.height < 18 {
        frame.render_widget(
            Paragraph::new("Please resize the terminal to at least 45 × 18. Press q to quit.")
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(if transcript { 5 } else { 4 }),
        Constraint::Length(3),
        Constraint::Length(2),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(area);
    let sessions = app
        .history
        .entries
        .iter()
        .map(|e| (&e.source, &e.session_id))
        .collect::<HashSet<_>>()
        .len();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " CODEX ",
                    Style::default()
                        .fg(Color::Black)
                        .bg(ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw(if transcript {
                    "  SESSION HISTORY"
                } else {
                    "  PROMPT HISTORY"
                }),
            ]),
            Line::from(Span::styled(
                if transcript {
                    format!(
                        " {} messages / tool events · {} malformed lines skipped · Esc back",
                        app.history.entries.len(),
                        app.history.skipped
                    )
                } else {
                    format!(
                        " {} prompts · {} sessions · {} malformed lines skipped",
                        app.history.entries.len(),
                        sessions,
                        app.history.skipped
                    )
                },
                Style::default().fg(MUTED),
            )),
        ]),
        rows[0],
    );

    if transcript {
        frame.render_widget(
            Paragraph::new(display_text(&app.session_info)).block(panel(" Session details ")),
            rows[1],
        );
    } else {
        let today = Local::now().date_naive();
        let mut activity = [0u64; 30];
        for &index in &app.visible {
            if let Some(dt) = chrono::DateTime::from_timestamp(app.history.entries[index].ts, 0) {
                let age = today
                    .signed_duration_since(dt.with_timezone(&Local).date_naive())
                    .num_days();
                if (0..30).contains(&age) {
                    activity[29 - age as usize] += 1;
                }
            }
        }
        let start = today.checked_sub_days(Days::new(29)).unwrap_or(today);
        let block = panel(format!(
            " Activity · {start} → {today} · {} matching prompts ",
            activity.iter().sum::<u64>()
        ));
        let width = block.inner(rows[1]).width as usize;
        // Repeat each day's height across its share of the available columns.
        // Keep the original daily counts for the total and vertical scale.
        let bars: Vec<u64> = (0..width)
            .map(|column| activity[column * activity.len() / width])
            .collect();
        frame.render_widget(
            Sparkline::default()
                .block(block)
                .data(bars)
                .style(Style::default().fg(ACCENT)),
            rows[1],
        );
    }
    let search_title = if app.searching {
        " Search · typing · Enter to finish "
    } else {
        " Search · / to edit "
    };
    let placeholder = app.query.is_empty() && !app.searching;
    let query = if placeholder {
        if transcript {
            "Search messages, tools, or roles…"
        } else {
            "Search prompt text or session ID…"
        }
    } else {
        &app.query
    };
    // Keep the tail of long queries visible without splitting a Unicode character.
    let mut used = 0;
    let query_tail: String = display_text(query)
        .chars()
        .rev()
        .take_while(|c| {
            used += Span::raw(c.to_string()).width();
            used <= rows[2].width.saturating_sub(2) as usize
        })
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    frame.render_widget(
        Paragraph::new(query_tail)
            .block(
                panel(search_title).border_style(Style::default().fg(if app.searching {
                    ACCENT
                } else {
                    MUTED
                })),
            )
            .style(Style::default().fg(if placeholder { MUTED } else { Color::Reset })),
        rows[2],
    );

    let count = app.matched_count;
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(
                display_text(&app.filter_summary()),
                Style::default().fg(ACCENT),
            ),
            Line::styled(
                format!(
                    " {count}/{} records · Focus: {} · Tab switches panes",
                    app.history.entries.len(),
                    if app.focus == Focus::List {
                        "List"
                    } else {
                        "Details"
                    }
                ),
                Style::default().fg(MUTED),
            ),
        ]),
        rows[3],
    );

    let panes = Layout::default()
        .direction(if area.width >= 100 {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(rows[4]);
    let spacers: Vec<bool> = app
        .visible
        .iter()
        .enumerate()
        .map(|(position, &index)| {
            if !app.query.is_empty() {
                return true;
            }
            let group = app
                .groups
                .get_key_value(&index)
                .or_else(|| app.groups.iter().find(|(_, range)| range.contains(&index)));
            !group.is_some_and(|(id, range)| {
                app.expanded_groups.contains(id)
                    && app
                        .visible
                        .get(position + 1)
                        .is_some_and(|next| range.contains(next))
            })
        })
        .collect();
    let multiple_sources = app
        .history
        .entries
        .iter()
        .filter_map(|entry| entry.source.as_ref())
        .collect::<HashSet<_>>()
        .len()
        > 1;
    let items: Vec<ListItem> = app
        .visible
        .iter()
        .enumerate()
        .map(|(position, &i)| {
            if let Some(range) = app.groups.get(&i) {
                let failures = app.history.entries[range.clone()]
                    .iter()
                    .filter(|entry| crate::app::is_failed_tool(entry))
                    .count();
                return spaced_item(
                    vec![
                        Line::styled(
                            format!(
                                "{} Tool activity · {} entries{}",
                                if app.expanded_groups.contains(&i) {
                                    "▾"
                                } else {
                                    "▸"
                                },
                                range.len(),
                                if failures > 0 {
                                    format!(" · {failures} failed")
                                } else {
                                    String::new()
                                }
                            ),
                            Style::default()
                                .fg(if failures > 0 {
                                    Color::Red
                                } else {
                                    Color::Yellow
                                })
                                .add_modifier(Modifier::BOLD),
                        ),
                        Line::from(""),
                    ],
                    spacers[position],
                );
            }
            let entry = &app.history.entries[i];
            if app.tool_collapsed(i) {
                let summary = entry.text.lines().next().filter(|line| {
                    line.starts_with("Completed")
                        || line.starts_with("Failed")
                        || line.starts_with("Result received")
                        || *line == "No result recorded"
                });
                let lines = vec![Line::styled(
                    tool_label(entry, panes[0].width.saturating_sub(4) as usize),
                    Style::default().fg(if crate::app::is_failed_tool(entry) {
                        Color::Red
                    } else if summary.is_some_and(|s| s.starts_with("Completed")) {
                        Color::Green
                    } else {
                        Color::Yellow
                    }),
                )];
                return spaced_item(lines, spacers[position]);
            }
            let preview = display_text(
                &entry
                    .text
                    .chars()
                    .take(panes[0].width as usize * 2)
                    .collect::<String>(),
            )
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
            let session: String = if transcript {
                entry.session_id.clone()
            } else {
                let id: String = entry.session_id.chars().take(8).collect();
                if multiple_sources {
                    format!(
                        "{} · {id}",
                        entry
                            .source
                            .as_deref()
                            .map(crate::history::source_name)
                            .unwrap_or_default()
                    )
                } else {
                    id
                }
            };
            let lines = vec![
                Line::from(Span::styled(
                    format!(
                        "{}{} · {}",
                        if transcript && crate::app::is_tool(entry) {
                            "▾ "
                        } else {
                            ""
                        },
                        timestamp(entry.ts),
                        display_text(&session)
                    ),
                    Style::default().fg(if transcript {
                        match entry.session_id.as_str() {
                            "USER" => Color::Cyan,
                            "ASSISTANT" => Color::Green,
                            role if role.starts_with("TOOL") => Color::Yellow,
                            _ => MUTED,
                        }
                    } else {
                        MUTED
                    }),
                )),
                Line::from(if preview.is_empty() {
                    "(empty prompt)".into()
                } else {
                    preview
                }),
            ];
            spaced_item(lines, spacers[position])
        })
        .collect();
    let title = format!(
        " {} · {}/{} · {}{} ",
        if transcript { "Timeline" } else { "Prompts" },
        app.list.selected().map_or(0, |i| i + 1),
        app.visible.len(),
        if transcript {
            if app.oldest_first {
                "reverse"
            } else {
                "file order"
            }
        } else if app.oldest_first {
            "oldest"
        } else {
            "newest"
        },
        if app.session.is_some() {
            " · session filter"
        } else {
            ""
        }
    );
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new(if transcript {
                "No matching session entries.\n/ edits search · r reloads · Esc back"
            } else {
                "No matching prompts.\nEsc clears filters · r reloads"
            })
            .block(
                panel(title).border_style(Style::default().fg(if app.focus == Focus::List {
                    ACCENT
                } else {
                    MUTED
                })),
            )
            .wrap(Wrap { trim: false }),
            panes[0],
        );
    } else {
        let heights: Vec<usize> = items.iter().map(ListItem::height).collect();
        let block = panel(title)
            .border_style(Style::default().fg(if app.focus == Focus::List {
                ACCENT
            } else {
                MUTED
            }))
            .padding(Padding::new(0, 0, 1, 0));
        let inner = block.inner(panes[0]);
        frame.render_stateful_widget(
            List::new(items)
                .block(block)
                .highlight_symbol("▎ ")
                .repeat_highlight_symbol(true)
                .highlight_style(Style::default().bg(Color::Rgb(28, 47, 62)).fg(Color::White)),
            panes[0],
            &mut app.list,
        );
        // The spacer belongs between items, not inside the selected content.
        // Keep one shared gap while giving the highlight balanced boundaries.
        if let Some(selected) = app.list.selected()
            && selected >= app.list.offset()
            && spacers[selected]
        {
            let bottom = heights[app.list.offset()..=selected].iter().sum::<usize>();
            if bottom > 0 && bottom <= inner.height as usize {
                frame.render_widget(
                    Clear,
                    Rect::new(inner.x, inner.y + bottom as u16 - 1, inner.width, 1),
                );
            }
        }
    }

    let width = panes[1].width.saturating_sub(2).max(1) as usize;
    let mut lines = Vec::new();
    if let Some(range) = app.selected_group() {
        lines.push(Line::styled(
            format!("{} tool entries", range.len()),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        lines.push(Line::from(""));
        for text in [
            "Enter / Space: expand or collapse this group.",
            "Select a tool inside the group to view its input and result.",
            "c: collapse all tool groups.",
            "/: search includes all hidden tools.",
        ] {
            lines.extend(
                textwrap::wrap(text, width)
                    .into_iter()
                    .map(|line| Line::from(line.into_owned())),
            );
        }
        lines.push(Line::from(""));
        let mut counts = std::collections::BTreeMap::new();
        for entry in &app.history.entries[range.clone()] {
            *counts.entry(&entry.session_id).or_insert(0) += 1;
        }
        for (name, count) in counts {
            lines.extend(
                textwrap::wrap(&format!("{} × {count}", display_text(name)), width)
                    .into_iter()
                    .map(|line| Line::from(line.into_owned())),
            );
        }
    } else if let Some(entry) = app.selected() {
        if multiple_sources && let Some(source) = &entry.source {
            lines.extend(
                textwrap::wrap(
                    &format!("Source: {}", display_text(&source.display().to_string())),
                    width,
                )
                .into_iter()
                .map(|line| Line::styled(line.into_owned(), Style::default().fg(MUTED))),
            );
        }
        lines.push(Line::styled(
            timestamp(entry.ts),
            Style::default().fg(ACCENT),
        ));
        for line in textwrap::wrap(
            &format!(
                "{}: {}",
                if transcript { "Role" } else { "Session" },
                display_text(&entry.session_id)
            ),
            width,
        ) {
            lines.push(Line::styled(line.into_owned(), Style::default().fg(MUTED)));
        }
        lines.push(Line::from(""));
        let collapsed = app
            .list
            .selected()
            .and_then(|i| app.visible.get(i))
            .is_some_and(|&i| app.tool_collapsed(i));
        let body = if collapsed {
            let summary = entry
                .text
                .lines()
                .next()
                .filter(|line| {
                    line.starts_with("Completed")
                        || line.starts_with("Failed")
                        || line.starts_with("Result received")
                        || *line == "No result recorded"
                })
                .unwrap_or("Tool details collapsed.");
            format!(
                "{summary}\n\nPress Enter or Space to expand.\nInput and result appear together.\nSearch includes the hidden content."
            )
        } else {
            crate::formatting::typeset(&entry.text)
        };
        for line in display_text(&body).split('\n') {
            if line.is_empty() {
                lines.push(Line::from(""));
            } else {
                lines.extend(
                    textwrap::wrap(
                        line,
                        textwrap::Options::new(width).subsequent_indent(
                            &" ".repeat(
                                line.chars()
                                    .take_while(|c| *c == ' ')
                                    .count()
                                    .min(width.saturating_sub(1)),
                            ),
                        ),
                    )
                    .into_iter()
                    .map(|s| {
                        let style = if transcript && crate::app::is_tool(entry) {
                            if line.starts_with("FAILED")
                                || line.starts_with("Failed")
                                || line == "ERROR OUTPUT"
                            {
                                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                            } else if line.starts_with("COMPLETED") || line.starts_with("Completed")
                            {
                                Style::default()
                                    .fg(Color::Green)
                                    .add_modifier(Modifier::BOLD)
                            } else if matches!(line, "OUTPUT" | "INPUT" | "RESULT") {
                                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
                            } else if line == "DETAILS" || line == "CALL REFERENCE" {
                                Style::default().fg(MUTED)
                            } else {
                                Style::default()
                            }
                        } else {
                            Style::default()
                        };
                        Line::styled(s.into_owned(), style)
                    }),
                );
            }
        }
    } else {
        lines.push(Line::from("Select a prompt to read it here."));
    }
    let height = panes[1].height.saturating_sub(2) as usize;
    app.detail_page_size = height.saturating_sub(1).max(1);
    app.detail_max_scroll = lines.len().saturating_sub(height);
    app.scroll = app.scroll.min(app.detail_max_scroll);
    let title = format!(
        " {} · line {} · ←/→ scroll ",
        if transcript {
            "Details"
        } else {
            "Prompt · Enter opens session"
        },
        app.scroll + 1
    );
    frame.render_widget(
        Paragraph::new(
            lines
                .into_iter()
                .skip(app.scroll)
                .take(height)
                .collect::<Vec<_>>(),
        )
        .block(panel(title).border_style(Style::default().fg(
            if app.focus == Focus::Detail {
                ACCENT
            } else {
                MUTED
            },
        ))),
        panes[1],
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(if transcript {
                " Tab focus  [/] failed tools  / search  Enter fold  c fold all  Esc back  ? help  q quit"
            } else {
                " Tab focus  / search  x clear  s session  t source  Enter open  ? help  q quit"
            }),
            Line::styled(display_text(&app.status), Style::default().fg(MUTED)),
        ]),
        rows[5],
    );
    if app.help {
        help(frame, area, transcript);
    }
}

fn help(frame: &mut Frame, area: Rect, transcript: bool) {
    let width = area.width.min(64);
    let height = area.height.min(24);
    let popup = Rect::new(
        (area.width - width) / 2,
        (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    let shortcuts = [
        (
            if transcript { "Enter / Space" } else { "Enter" },
            if transcript {
                "Expand / collapse tool details"
            } else {
                "Open selected session history"
            },
        ),
        ("Tab / Shift+Tab", "Switch list / details focus"),
        ("↑/↓ or j/k", "Navigate focused pane"),
        ("Home/End g/G", "First / last item or detail line"),
        ("PgUp/PgDn", "Page through focused pane"),
        ("←/→ or K/J", "Scroll prompt preview"),
        ("/", "Edit live search"),
        ("x", "Clear search"),
        ("t", "Cycle source (prompt history)"),
        (
            "c",
            if transcript {
                "Collapse all tool groups"
            } else {
                "Available in session history"
            },
        ),
        ("Enter / Esc", "Finish search (keep query)"),
        ("Ctrl+U", "Clear query while typing"),
        (
            "s",
            if transcript {
                "Available in prompt history"
            } else {
                "Toggle selected session filter"
            },
        ),
        ("[ / ]", "Previous / next failed tool"),
        ("o", "Reverse chronological order"),
        ("r", "Reload history from disk"),
        (
            "Esc",
            if transcript {
                "Clear next filter, then go back"
            } else {
                "Clear search, session, then source"
            },
        ),
        ("q / Ctrl+C", "Quit"),
    ];
    let key_width = shortcuts
        .iter()
        .map(|(key, _)| Span::raw(*key).width())
        .max()
        .unwrap_or(0);
    let mut lines: Vec<Line> = shortcuts
        .into_iter()
        .map(|(key, description)| {
            let padding = " ".repeat(key_width - Span::raw(key).width() + 2);
            Line::from(format!("{key}{padding}{description}"))
        })
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(
        "Timestamps use local time. Any key closes help.",
    ));
    frame.render_widget(
        Paragraph::new(lines)
            .block(panel(" Keyboard shortcuts "))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Entry, History};
    use ratatui::{Terminal, backend::TestBackend};
    #[test]
    fn tool_summary_leaves_room_for_failure_status() {
        let entry = Entry {
            tool: Some(crate::history::ToolInfo {
                summary: "cargo test 中文路径".repeat(20),
                failed: true,
            }),
            source: None,
            session_id: "TOOL · shell".into(),
            ts: 0,
            text: "Failed · exit 2".into(),
        };
        let label = tool_label(&entry, 52);
        assert!(label.contains("cargo test"));
        assert!(label.contains('…'));
        assert!(label.ends_with("Failed · exit 2"));
        assert!(Span::raw(label).width() <= 52);
    }
    #[test]
    fn grouped_tools_are_adjacent_and_selected_content_is_not_cleared() {
        let mut app = App::from_session(crate::session::Session {
            history: History {
                entries: vec![
                    Entry {
                        tool: None,
                        source: None,
                        session_id: "USER".into(),
                        ts: 1,
                        text: "hello".into(),
                    },
                    Entry {
                        tool: None,
                        source: None,
                        session_id: "TOOL · shell".into(),
                        ts: 2,
                        text: "Completed".into(),
                    },
                    Entry {
                        tool: None,
                        source: None,
                        session_id: "TOOL · search".into(),
                        ts: 3,
                        text: "Completed".into(),
                    },
                    Entry {
                        tool: None,
                        source: None,
                        session_id: "ASSISTANT".into(),
                        ts: 4,
                        text: "done".into(),
                    },
                ],
                skipped: 0,
            },
            info: String::new(),
            path: "demo".into(),
        });
        app.move_selection(1);
        app.toggle_tool();
        app.move_selection(1);
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let rows: Vec<String> = terminal
            .backend()
            .buffer()
            .content
            .chunks(120)
            .map(|row| row.iter().map(|cell| cell.symbol()).collect())
            .collect();
        let row = |text: &str| rows.iter().position(|line| line.contains(text)).unwrap();
        assert_eq!(row("shell · Completed"), row("Tool activity") + 1);
        assert_eq!(row("search · Completed"), row("shell · Completed") + 1);
        assert_eq!(row("ASSISTANT"), row("search · Completed") + 2);
    }
    #[test]
    fn tool_details_are_hidden_until_expanded() {
        let mut app = App::new(History {
            entries: vec![Entry {
                tool: None,
                source: None,
                session_id: "TOOL · shell".into(),
                ts: 1,
                text: crate::formatting::typeset(r#"{"command":"echo hello\nwhoami"}"#),
            }],
            skipped: 0,
        });
        app.session_source = Some("demo.jsonl".into());
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let screen = |terminal: &Terminal<TestBackend>| -> String {
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect()
        };
        assert!(screen(&terminal).contains("Tool details collapsed"));
        assert!(!screen(&terminal).contains("echo hello"));
        app.query = "whoami".into();
        app.filter();
        assert_eq!(app.visible.len(), 1);
        app.toggle_tool();
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        assert!(screen(&terminal).contains("echo hello"));
        assert!(screen(&terminal).contains("command:"));
        assert!(!screen(&terminal).contains("{\"command\""));
        app.toggle_tool();
        assert!(app.tool_collapsed(0));
    }
    #[test]
    fn renders_empty_unicode_and_small_terminals() {
        for (width, height) in [(120, 35), (60, 24), (20, 5)] {
            let mut app = App::new(History::default());
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            let rendered: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            assert!(rendered.contains(if width >= 45 {
                "No matching prompts"
            } else {
                "Please resize"
            }));
            app.history.entries.push(Entry {
                tool: None,
                source: None,
                session_id: "世界".into(),
                ts: i64::MAX,
                text: "你好\nRust 🦀\n".repeat(100),
            });
            app.filter();
            app.scroll = usize::MAX;
            terminal.draw(|f| draw(f, &mut app)).unwrap();
            if width >= 45 {
                assert!(app.scroll > 0 && app.scroll < 400);
            }
            app.help = true;
            terminal.draw(|f| draw(f, &mut app)).unwrap();
        }
    }
}
