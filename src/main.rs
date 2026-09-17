mod activity;
mod app;
mod clipboard;
mod detail_find;
mod formatting;
mod history;
mod loader;
mod search;
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
    /// Clipboard backend: auto uses native locally and terminal OSC52 over SSH
    #[arg(long, value_enum, default_value_t = clipboard::Mode::Auto)]
    clipboard: clipboard::Mode,
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
    let mut app = App::new(History::default());
    app.query = args.query;
    app.filter();
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("An interactive terminal is required. Run codex-prompt-history in a terminal.");
    }
    let mut loader = loader::Loader::new()?;
    let mut clipboard = clipboard::Clipboard::new(args.clipboard)?;
    start_load(&mut app, &mut loader, loader::Job::History(paths.clone()));
    let mut terminal = ratatui::init();
    let paste_enabled = crossterm::execute!(io::stdout(), event::EnableBracketedPaste).is_ok();
    let result = run(
        &mut terminal,
        &mut app,
        &paths,
        args.sessions_dir.as_deref(),
        &mut loader,
        &mut clipboard,
    );
    if paste_enabled {
        let _ = crossterm::execute!(io::stdout(), event::DisableBracketedPaste);
    }
    ratatui::restore();
    result
}

fn run(
    terminal: &mut ratatui::DefaultTerminal,
    root: &mut App,
    paths: &[PathBuf],
    sessions_dir: Option<&std::path::Path>,
    loader: &mut loader::Loader,
    clipboard: &mut clipboard::Clipboard,
) -> Result<()> {
    loop {
        apply_load_events(root, loader);
        if let Some(outcome) = clipboard.poll() {
            let message = match outcome {
                clipboard::Outcome::Native(label) => format!("Copied {label} to system clipboard"),
                clipboard::Outcome::Terminal(selection) => {
                    match clipboard::terminal_copy(&mut io::stdout(), &selection.text) {
                        Ok(()) => format!(
                            "Copy request sent for {} (OSC52; terminal support required)",
                            selection.label
                        ),
                        Err(error) => format!("Copy failed: {error}"),
                    }
                }
                clipboard::Outcome::Error(error) => format!("Copy failed: {error}"),
            };
            match root.transcript.as_deref_mut() {
                Some(app) => app.status = message,
                None => root.status = message,
            };
        }
        terminal.draw(|frame| match root.transcript.as_deref_mut() {
            Some(app) => ui::draw(frame, app),
            None => ui::draw(frame, root),
        })?;
        if !event::poll(Duration::from_millis(200))? {
            continue;
        }
        let event = event::read()?;
        if let Event::Paste(text) = &event {
            let app = match root.transcript.as_deref_mut() {
                Some(app) => app,
                None => &mut *root,
            };
            if !app.help {
                if app.find.editing {
                    app.find.editor.insert(&mut app.find.query, text);
                    app.find.changed();
                } else if app.searching {
                    app.search.insert(&mut app.query, text);
                    app.filter();
                }
            }
            continue;
        }
        let Event::Key(key) = event else {
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
            if !current.is_editing() && !current.help {
                if loader.pending.is_some() {
                    loader.cancel();
                    current.loading = false;
                    current.status = "Loading cancelled".into();
                    root.loading = false;
                    continue;
                }
                if current.focus == app::Focus::Activity {
                    current.focus = app::Focus::List;
                    continue;
                }
                if current.focus == app::Focus::Detail && !current.find.query.is_empty() {
                    current.find.clear();
                    continue;
                }
                if current.clear_one_filter() {
                    continue;
                }
                if root.transcript.is_some() {
                    root.transcript = None;
                    continue;
                }
            }
        }
        if key.code == KeyCode::Enter
            && root.transcript.is_none()
            && !root.is_editing()
            && !root.help
        {
            if root.focus == app::Focus::Activity {
                root.apply_activity_date();
                continue;
            }
            if let Some(entry) = root.selected() {
                let directory = session_directory(entry, sessions_dir);
                let id = entry.session_id.clone();
                start_load(root, loader, loader::Job::OpenSession { directory, id });
            }
            continue;
        }
        let ready_for_reload = root.transcript.as_deref().unwrap_or(root);
        if key.code == KeyCode::Char('r')
            && !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && !ready_for_reload.is_editing()
            && !ready_for_reload.help
        {
            let job = if let Some(path) = &ready_for_reload.session_source {
                loader::Job::ReloadSession(path.clone())
            } else {
                loader::Job::History(paths.to_vec())
            };
            start_load(root, loader, job);
            continue;
        }
        let app = match root.transcript.as_deref_mut() {
            Some(app) => app,
            None => root,
        };
        if app.help {
            match key.code {
                KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q' | '?') => app.help = false,
                KeyCode::Down | KeyCode::Char('j') => {
                    app.help_scroll = app.help_scroll.saturating_add(1).min(app.help_max_scroll)
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    app.help_scroll = app.help_scroll.saturating_sub(1)
                }
                KeyCode::PageDown => {
                    app.help_scroll = app
                        .help_scroll
                        .saturating_add(app.help_page)
                        .min(app.help_max_scroll)
                }
                KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(app.help_page),
                KeyCode::Home => app.help_scroll = 0,
                KeyCode::End => app.help_scroll = app.help_max_scroll,
                _ => {}
            }
            continue;
        }
        if app.is_editing() {
            handle_search_key(app, key);
            continue;
        }
        if key.code == KeyCode::Char('f') && key.modifiers.contains(KeyModifiers::CONTROL) {
            app.begin_search();
            continue;
        }
        if key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            continue;
        }
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Char('C') => {
                let kind = match key.code {
                    KeyCode::Char('Y') => clipboard::Kind::SessionId,
                    KeyCode::Char('C') => clipboard::Kind::Command,
                    _ => clipboard::Kind::Content,
                };
                if clipboard.pending {
                    app.status = "A clipboard request is already in progress".into();
                } else {
                    match clipboard::selection(app, kind)
                        .and_then(|selection| clipboard.copy(selection))
                    {
                        Ok(()) => app.status = "Copying…".into(),
                        Err(error) => app.status = format!("Copy unavailable: {error}"),
                    }
                }
            }
            KeyCode::Char('c') if app.session_source.is_some() => app.collapse_groups(),
            KeyCode::Char(']') if app.session_source.is_some() => app.jump_failure(true),
            KeyCode::Char('[') if app.session_source.is_some() => app.jump_failure(false),
            KeyCode::Enter | KeyCode::Char(' ') if app.session_source.is_some() => {
                app.toggle_tool()
            }
            KeyCode::Char('q') => break,
            KeyCode::Char('f') => app.begin_find(),
            KeyCode::Char('F') => app.find.clear(),
            KeyCode::Char('n') => app.jump_detail_match(true),
            KeyCode::Char('N') => app.jump_detail_match(false),
            KeyCode::Char('a') if app.session_source.is_none() => app.focus_activity(),
            KeyCode::Char('w') if app.session_source.is_none() => app.cycle_activity_window(),
            KeyCode::Char('d') if app.session_source.is_none() => {
                app.date_filter = None;
                app.filter();
            }
            KeyCode::Left if app.focus == app::Focus::Activity => app.navigate(-1),
            KeyCode::Right if app.focus == app::Focus::Activity => app.navigate(1),
            KeyCode::Tab | KeyCode::BackTab => app.toggle_focus(),
            KeyCode::Char('t') if app.session_source.is_none() => app.cycle_source(),
            KeyCode::Char('x') => {
                app.query.clear();
                app.filter();
            }
            KeyCode::Char('/') => app.begin_search(),
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
            KeyCode::Char('?') | KeyCode::F(1) => {
                app.help = true;
                app.help_scroll = 0;
            }
            _ => {}
        }
    }
    Ok(())
}

fn handle_search_key(app: &mut App, key: event::KeyEvent) {
    match key.code {
        KeyCode::F(1) => {
            app.help = true;
            app.help_scroll = 0;
            return;
        }
        KeyCode::Esc => {
            if app.find.editing {
                if let Some((scroll, focus)) = app.find.cancel() {
                    app.scroll = scroll;
                    app.focus = focus;
                }
            } else {
                app.cancel_search();
            }
            return;
        }
        KeyCode::Enter => {
            if app.find.editing {
                app.find.confirm();
            } else {
                app.confirm_search();
            }
            return;
        }
        _ => {}
    }
    if app.find.editing {
        if edit_query(&mut app.find.editor, &mut app.find.query, key) {
            app.find.changed();
        }
    } else if edit_query(&mut app.search, &mut app.query, key) {
        app.filter();
    }
}

fn edit_query(editor: &mut search::Editor, query: &mut String, key: event::KeyEvent) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Left => {
            editor.left(query);
            false
        }
        KeyCode::Right => {
            editor.right(query);
            false
        }
        KeyCode::Home => {
            editor.home();
            false
        }
        KeyCode::End => {
            editor.end(query);
            false
        }
        KeyCode::Char('a') if control => {
            editor.home();
            false
        }
        KeyCode::Char('e') if control => {
            editor.end(query);
            false
        }
        KeyCode::Char('u') if control => {
            editor.clear(query);
            true
        }
        KeyCode::Char('w') if control => {
            editor.delete_word(query);
            true
        }
        KeyCode::Backspace if control => {
            editor.delete_word(query);
            true
        }
        KeyCode::Backspace => {
            editor.backspace(query);
            true
        }
        KeyCode::Delete => {
            editor.delete(query);
            true
        }
        KeyCode::Up => {
            editor.recall(query, true);
            true
        }
        KeyCode::Down => {
            editor.recall(query, false);
            true
        }
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            editor.insert(query, &c.to_string());
            true
        }
        _ => false,
    }
}

fn start_load(root: &mut App, loader: &mut loader::Loader, job: loader::Job) {
    let target = job.target();
    root.loading = false;
    if let Some(app) = root.transcript.as_deref_mut() {
        app.loading = false;
    }
    let result = loader.start(job);
    let app = if target == loader::Target::ReloadSession {
        root.transcript
            .as_deref_mut()
            .expect("reload requires a session")
    } else {
        root
    };
    match result {
        Ok(()) => {
            app.loading = true;
            app.status = "Loading… Esc cancels · existing view stays usable".into();
        }
        Err(error) => app.status = format!("Load failed: {error:#}"),
    }
}

fn apply_load_events(root: &mut App, loader: &mut loader::Loader) {
    while let Some(event) = loader.poll() {
        match event {
            loader::Event::Progress { target, message } => {
                let app = if target == loader::Target::ReloadSession {
                    root.transcript.as_deref_mut()
                } else {
                    Some(&mut *root)
                };
                if let Some(app) = app {
                    app.status = message;
                }
            }
            loader::Event::Finished { target, result } => {
                root.loading = false;
                if let Some(app) = root.transcript.as_deref_mut() {
                    app.loading = false;
                }
                match result {
                    Ok(loader::Loaded::History(history)) => {
                        root.replace_history(history);
                        root.status = "History reloaded".into();
                    }
                    Ok(loader::Loaded::Session { session, cached })
                        if target == loader::Target::OpenSession =>
                    {
                        let mut app = App::from_session(session);
                        app.search.history = root.search.history.clone();
                        app.status = if cached {
                            "Session loaded from memory cache"
                        } else {
                            "Session loaded"
                        }
                        .into();
                        root.transcript = Some(Box::new(app));
                        root.status.clear();
                    }
                    Ok(loader::Loaded::Session { session, .. }) => {
                        if let Some(app) = root
                            .transcript
                            .as_deref_mut()
                            .filter(|app| app.session_source.as_ref() == Some(&session.path))
                        {
                            app.replace_history(session.history);
                            app.session_id = (!session.id.is_empty()).then_some(session.id);
                            app.session_info = session.info;
                            app.status = "Session reloaded".into();
                        }
                    }
                    Err(error) => {
                        let app = if target == loader::Target::ReloadSession {
                            root.transcript.as_deref_mut()
                        } else {
                            Some(&mut *root)
                        };
                        if let Some(app) = app {
                            app.status = format!("Load failed: {error}");
                        }
                    }
                }
            }
        }
    }
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
