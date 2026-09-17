//! Explicit keyboard-triggered copy operations. No shell commands or temp files.
use crate::{app::App, history::RecordedCommand};
use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use crossterm::{Command, clipboard::CopyToClipboard};
use std::{io::Write, sync::mpsc, thread};

#[derive(Clone, Copy, Debug, Default, ValueEnum, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Native,
    Terminal,
}
#[derive(Clone, Copy)]
pub enum Kind {
    Content,
    SessionId,
    Command,
}

pub struct Selection {
    pub label: &'static str,
    pub text: String,
}
pub fn selection(app: &App, kind: Kind) -> Result<Selection> {
    match kind {
        Kind::Content => Ok(Selection {
            label: "record content",
            text: app
                .selected()
                .context("Select an individual record to copy")?
                .text
                .clone(),
        }),
        Kind::SessionId => {
            let id = if app.session_source.is_some() {
                app.session_id.as_deref()
            } else {
                app.selected().map(|entry| entry.session_id.as_str())
            };
            Ok(Selection {
                label: "session ID",
                text: id
                    .filter(|id| !id.is_empty())
                    .context("No session ID is recorded for this selection")?
                    .to_owned(),
            })
        }
        Kind::Command => {
            let command = app
                .selected()
                .and_then(|entry| entry.tool.as_ref())
                .and_then(|tool| tool.command.as_ref())
                .context("The selected record has no recorded command")?;
            match command {
                RecordedCommand::Shell(text) => Ok(Selection {
                    label: "tool command",
                    text: text.clone(),
                }),
                RecordedCommand::Arguments(args) => Ok(Selection {
                    label: "tool arguments (JSON array)",
                    text: serde_json::to_string(args)?,
                }),
            }
        }
    }
}

pub enum Outcome {
    Native(&'static str),
    Terminal(Selection),
    Error(String),
}

pub struct Clipboard {
    requests: mpsc::SyncSender<Selection>,
    results: mpsc::Receiver<Outcome>,
    pub pending: bool,
}
impl Clipboard {
    pub fn new(mode: Mode) -> Result<Self> {
        let (requests, receiver) = mpsc::sync_channel::<Selection>(1);
        let (sender, results) = mpsc::channel();
        // Native clipboard access on an SSH host targets the wrong machine.
        let remote =
            std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some();
        thread::Builder::new()
            .name("clipboard".into())
            .spawn(move || {
                let mut clipboard = None;
                while let Ok(selection) = receiver.recv() {
                    let result = if selection.text.contains('\0') {
                        Outcome::Error("Cannot copy text containing NUL bytes".into())
                    } else if mode == Mode::Terminal || mode == Mode::Auto && remote {
                        Outcome::Terminal(selection)
                    } else {
                        let copied = (|| -> Result<()> {
                            if clipboard.is_none() {
                                clipboard = Some(arboard::Clipboard::new()?);
                            }
                            clipboard
                                .as_mut()
                                .unwrap()
                                .set_text(selection.text.as_str())?;
                            Ok(())
                        })();
                        match copied {
                            Ok(()) => Outcome::Native(selection.label),
                            Err(error) if mode == Mode::Native => {
                                Outcome::Error(format!("Native clipboard unavailable: {error}"))
                            }
                            Err(_) => {
                                clipboard = None;
                                Outcome::Terminal(selection)
                            }
                        }
                    };
                    if sender.send(result).is_err() {
                        break;
                    }
                }
                // Keep the native clipboard object alive for Linux selection ownership.
            })
            .context("Cannot start clipboard worker")?;
        Ok(Self {
            requests,
            results,
            pending: false,
        })
    }

    pub fn copy(&mut self, selection: Selection) -> Result<()> {
        if self.pending {
            bail!("A clipboard request is already in progress");
        }
        self.requests
            .try_send(selection)
            .context("Clipboard worker unavailable")?;
        self.pending = true;
        Ok(())
    }

    pub fn poll(&mut self) -> Option<Outcome> {
        match self.results.try_recv() {
            Ok(result) => {
                self.pending = false;
                Some(result)
            }
            Err(mpsc::TryRecvError::Disconnected) if self.pending => {
                self.pending = false;
                Some(Outcome::Error("Clipboard worker stopped".into()))
            }
            _ => None,
        }
    }
}

pub fn terminal_copy(writer: &mut impl Write, text: &str) -> Result<()> {
    // OSC52 has terminal-specific limits; never silently truncate a long copy.
    if text.len() > 100 * 1024 {
        bail!("Text exceeds the 100 KiB terminal-copy limit; use --clipboard native");
    }
    let mut sequence = String::new();
    CopyToClipboard::to_clipboard_from(text).write_ansi(&mut sequence)?;
    writer.write_all(sequence.as_bytes())?;
    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::{Entry, History, ToolInfo};
    #[test]
    fn content_and_commands_are_complete_and_ids_are_not_role_names() {
        let full = "echo '中文'\nprintf '%s' \"$HOME\"\n".repeat(20);
        let mut app = App::new(History {
            sources: Vec::new(),
            entries: vec![Entry {
                source: None,
                tool: Some(ToolInfo {
                    summary: "shortened…".into(),
                    failed: false,
                    command: Some(RecordedCommand::Shell(full.clone())),
                }),
                session_id: "TOOL · shell".into(),
                ts: 0,
                text: full.clone(),
            }],
            skipped: 0,
        });
        assert_eq!(selection(&app, Kind::Content).unwrap().text, full);
        assert_eq!(selection(&app, Kind::Command).unwrap().text, full);
        app.session_source = Some("session.jsonl".into());
        app.session_id = Some("real-session-id".into());
        assert_eq!(
            selection(&app, Kind::SessionId).unwrap().text,
            "real-session-id"
        );
        app.visible.clear();
        app.list.select(None);
        assert!(selection(&app, Kind::Content).is_err());
        assert_eq!(
            selection(&app, Kind::SessionId).unwrap().text,
            "real-session-id"
        );
    }
    #[test]
    fn terminal_copy_encodes_and_never_truncates() {
        let mut out = Vec::new();
        terminal_copy(&mut out, "hello").unwrap();
        assert!(String::from_utf8(out).unwrap().contains("52;c;aGVsbG8="));
        let mut out = Vec::new();
        assert!(terminal_copy(&mut out, &"x".repeat(100 * 1024 + 1)).is_err());
        assert!(out.is_empty());
    }
}
