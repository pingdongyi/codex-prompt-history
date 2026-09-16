mod app;
mod formatting;
mod history;
mod session;
mod tool_summary;
mod ui;

use anyhow::{Context, Result, bail};
use app::App;
use clap::Parser;
use history::History;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::{
    io::{self, IsTerminal},
    path::PathBuf,
    time::Duration,
};

#[derive(Parser)]
#[command(
    version,
    about = "Browse, search, and visualize your Codex prompt history"
)]
struct Args {
    /// History JSONL file; repeat to merge files (default: ~/.codex/history.jsonl)
    #[arg(short, long)]
    file: Vec<PathBuf>,
    /// Start with a case-insensitive search
    #[arg(short, long, default_value = "")]
    query: String,
    /// Session log directory (defaults to sessions beside the history file)
    #[arg(long)]
    sessions_dir: Option<PathBuf>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let paths = if args.file.is_empty() {
        vec![
            PathBuf::from(
                std::env::var_os("HOME")
                    .or_else(|| std::env::var_os("USERPROFILE"))
                    .context("Home directory unavailable; specify --file PATH")?,
            )
            .join(".codex/history.jsonl"),
        ]
    } else {
        args.file
    };
    let mut app = App::new(History::load_many(&paths)?);
    app.query = args.query;
    app.filter();
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("An interactive terminal is required. Run codex-prompt-history in a terminal.");
    }
    let mut terminal = ratatui::init();
    let result = run(
        &mut terminal,
        &mut app,
        &paths,
        args.sessions_dir.as_deref(),
    );
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    root: &mut App,
    paths: &[PathBuf],
    sessions_dir: Option<&std::path::Path>,
) -> Result<()> {
    loop {
        terminal.draw(|frame| match root.transcript.as_deref_mut() {
            Some(app) => ui::draw(frame, app),
            None => ui::draw(frame, root),
        })?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        if key.code == KeyCode::Esc {
            let current = match root.transcript.as_deref_mut() {
                Some(app) => app,
                None => &mut *root,
            };
            if !current.searching && !current.help {
                if current.clear_one_filter() {
                    continue;
                }
                if root.transcript.is_some() {
                    root.transcript = None;
                    continue;
                }
            }
        }
        if key.code == KeyCode::Enter && root.transcript.is_none() && !root.searching && !root.help
        {
            if let Some(entry) = root.selected() {
                let directory = session_directory(entry, sessions_dir);
                match session::find(&directory, &entry.session_id)
                    .and_then(|path| session::Session::load(&path))
                {
                    Ok(session) => root.transcript = Some(Box::new(App::from_session(session))),
                    Err(error) => root.status = format!("Cannot open session: {error}"),
                }
            }
            continue;
        }
        let app = match root.transcript.as_deref_mut() {
            Some(app) => app,
            None => root,
        };
        if app.help {
            app.help = false;
            continue;
        }
        if app.searching {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => app.searching = false,
                KeyCode::Backspace => {
                    app.query.pop();
                    app.filter();
                }
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    app.query.clear();
                    app.filter();
                }
                KeyCode::Char(c)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    app.query.push(c);
                    app.filter();
                }
                _ => {}
            }
            continue;
        }
        match key.code {
            KeyCode::Char('c') if app.session_source.is_some() => app.collapse_groups(),
            KeyCode::Char(']') if app.session_source.is_some() => app.jump_failure(true),
            KeyCode::Char('[') if app.session_source.is_some() => app.jump_failure(false),
            KeyCode::Enter | KeyCode::Char(' ') if app.session_source.is_some() => {
                app.toggle_tool()
            }
            KeyCode::Char('q') => break,
            KeyCode::Tab | KeyCode::BackTab => app.toggle_focus(),
            KeyCode::Char('t') if app.session_source.is_none() => app.cycle_source(),
            KeyCode::Char('x') => {
                app.query.clear();
                app.filter();
            }
            KeyCode::Char('/') => app.searching = true,
            KeyCode::Esc => {}
            KeyCode::Char('j') | KeyCode::Down => app.navigate(1),
            KeyCode::Char('k') | KeyCode::Up => app.navigate(-1),
            KeyCode::Char('g') | KeyCode::Home => app.navigate(isize::MIN),
            KeyCode::Char('G') | KeyCode::End => app.navigate(isize::MAX),
            KeyCode::PageDown => app.page(1),
            KeyCode::PageUp => app.page(-1),
            KeyCode::Char('J') | KeyCode::Right => app.scroll = app.scroll.saturating_add(3),
            KeyCode::Char('K') | KeyCode::Left => app.scroll = app.scroll.saturating_sub(3),
            KeyCode::Char('s') if app.session_source.is_none() => app.toggle_session(),
            KeyCode::Char('o') => {
                app.oldest_first = !app.oldest_first;
                app.filter();
            }
            KeyCode::Char('?') => app.help = true,
            KeyCode::Char('r') if app.session_source.is_some() => {
                match session::Session::load(app.session_source.as_ref().unwrap()) {
                    Ok(session) => {
                        app.replace_history(session.history);
                        app.session_info = session.info;
                        app.status = "Session reloaded".into();
                    }
                    Err(error) => app.status = format!("Reload failed: {error}"),
                }
            }
            KeyCode::Char('r') => match History::load_many(paths) {
                Ok(history) => {
                    app.replace_history(history);
                    app.status = "History reloaded".into();
                }
                Err(error) => app.status = format!("Reload failed: {error}"),
            },
            _ => {}
        }
    }
    Ok(())
}

fn session_directory(entry: &history::Entry, override_dir: Option<&std::path::Path>) -> PathBuf {
    override_dir
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| {
            entry
                .source
                .as_deref()
                .and_then(std::path::Path::parent)
                .unwrap_or(std::path::Path::new("."))
                .join("sessions")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_file_is_opt_in_and_sessions_follow_the_source() {
        assert!(Args::parse_from(["history"]).file.is_empty());
        let args = Args::parse_from([
            "history",
            "--file",
            ".codex/history.jsonl",
            "--file",
            ".codex-beta/history.jsonl",
            "--file",
            ".codex-alpha/history.jsonl",
        ]);
        assert_eq!(args.file.len(), 3);
        let entry = history::Entry {
            tool: None,
            source: Some(args.file[1].clone()),
            session_id: "same".into(),
            ts: 0,
            text: String::new(),
        };
        assert_eq!(
            session_directory(&entry, None),
            PathBuf::from(".codex-beta/sessions")
        );
        assert_eq!(
            session_directory(&entry, Some(std::path::Path::new("custom"))),
            PathBuf::from("custom")
        );
    }
}
