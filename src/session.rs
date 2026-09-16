use crate::history::{Entry, History};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Session {
    pub history: History,
    pub info: String,
    pub id: String,
    pub path: PathBuf,
}

fn string(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

#[cfg(test)]
pub fn find(root: &Path, id: &str) -> Result<PathBuf> {
    find_with(root, id, &mut |_, _| Ok(()))
}

pub fn find_with(
    root: &Path,
    id: &str,
    progress: &mut crate::loader::Progress<'_>,
) -> Result<PathBuf> {
    if id.is_empty() {
        bail!("Selected prompt has no session ID");
    }
    let mut dirs = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(dir) = dirs.pop() {
        progress("Scanning session directories", files.len())?;
        for item in fs::read_dir(&dir).with_context(|| format!("Cannot read {}", dir.display()))? {
            progress("Scanning session files", files.len())?;
            let item = item?;
            let kind = item.file_type()?;
            if kind.is_dir() {
                dirs.push(item.path());
            } else if kind.is_file() && item.path().extension().is_some_and(|ext| ext == "jsonl") {
                files.push(item.path());
            }
        }
    }
    files.sort();
    // Standard rollout names end in the session ID. Verify metadata too, so
    // custom filenames work and a filename cannot select the wrong session.
    files.sort_by_key(|p| {
        !p.file_stem()
            .is_some_and(|s| s.to_string_lossy().ends_with(id))
    });
    for (index, path) in files.into_iter().enumerate() {
        progress("Checking session IDs", index + 1)?;
        if matches_id(&path, id)? {
            return Ok(path);
        }
    }
    bail!("No session log for {id} in {}", root.display())
}

pub fn matches_id(path: &Path, id: &str) -> Result<bool> {
    let mut first = String::new();
    BufReader::new(File::open(path)?).read_line(&mut first)?;
    Ok(serde_json::from_str::<Value>(&first).is_ok_and(|value| {
        let payload = &value["payload"];
        value["type"] == "session_meta" && (payload["id"] == id || payload["session_id"] == id)
    }))
}

impl Session {
    pub fn load(path: &Path, progress: &mut crate::loader::Progress<'_>) -> Result<Self> {
        Self::parse_with(
            BufReader::new(
                File::open(path).with_context(|| format!("Cannot open {}", path.display()))?,
            ),
            path,
            progress,
        )
    }

    #[cfg(test)]
    fn parse(reader: impl BufRead, path: &Path) -> Result<Self> {
        Self::parse_with(reader, path, &mut |_, _| Ok(()))
    }

    fn parse_with(
        reader: impl BufRead,
        path: &Path,
        progress: &mut crate::loader::Progress<'_>,
    ) -> Result<Self> {
        let mut history = History::default();
        let mut info = String::new();
        let mut id = String::new();
        let mut model = String::new();
        let mut fallback = Vec::new();
        let mut has_messages = false;
        let mut calls = Vec::new();
        let mut results = Vec::new();
        for (index, line) in reader.lines().enumerate() {
            progress("Reading session lines", index + 1)?;
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let value: Value = match serde_json::from_str(&line) {
                Ok(value) => value,
                Err(_) => {
                    history.skipped += 1;
                    continue;
                }
            };
            let p = &value["payload"];
            let ts = chrono::DateTime::parse_from_rfc3339(&string(&value, "timestamp"))
                .map_or(0, |t| t.timestamp());
            match value["type"].as_str().unwrap_or_default() {
                "session_meta" => {
                    id = p["id"]
                        .as_str()
                        .filter(|id| !id.is_empty())
                        .or(p["session_id"].as_str())
                        .unwrap_or_default()
                        .to_owned();
                    info = format!(
                        "Session: {}\nDirectory: {}",
                        p["id"]
                            .as_str()
                            .or(p["session_id"].as_str())
                            .unwrap_or("unknown"),
                        string(p, "cwd")
                    )
                }
                "turn_context" => {
                    let next = string(p, "model");
                    if !next.is_empty() {
                        model = next;
                    }
                }
                "response_item" => {
                    let kind = p["type"].as_str().unwrap_or_default();
                    let mut tool = None;
                    let (role, text) = match kind {
                        "message" => {
                            let role = string(p, "role");
                            if role != "user" && role != "assistant" {
                                continue;
                            }
                            has_messages = true;
                            let content = p["content"]
                                .as_array()
                                .map(|parts| {
                                    parts
                                        .iter()
                                        .map(|part| {
                                            part["text"]
                                                .as_str()
                                                .map(crate::formatting::typeset)
                                                .unwrap_or_else(|| {
                                                    format!(
                                                        "[{}]",
                                                        part["type"]
                                                            .as_str()
                                                            .unwrap_or("attachment")
                                                    )
                                                })
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                })
                                .unwrap_or_default();
                            (role.to_uppercase(), content)
                        }
                        "function_call" | "custom_tool_call" => {
                            let name = string(p, "name");
                            let call_id = string(p, "call_id");
                            calls.push((history.entries.len(), call_id));
                            tool = Some(crate::history::ToolInfo {
                                command: p
                                    .get("arguments")
                                    .or_else(|| p.get("input"))
                                    .and_then(crate::tool_summary::recorded_command),
                                summary: p
                                    .get("arguments")
                                    .or_else(|| p.get("input"))
                                    .map(crate::tool_summary::summarize)
                                    .unwrap_or_default(),
                                failed: false,
                            });
                            (
                                format!("TOOL · {name}"),
                                p.get("arguments")
                                    .or_else(|| p.get("input"))
                                    .map(render_value)
                                    .unwrap_or_default(),
                            )
                        }
                        "function_call_output" | "custom_tool_call_output" => {
                            let call_id = string(p, "call_id");
                            let failed = p.get("output").is_some_and(crate::tool_summary::failed);
                            tool = Some(crate::history::ToolInfo {
                                command: None,
                                summary: String::new(),
                                failed,
                            });
                            let mut summary = p
                                .get("output")
                                .map(crate::formatting::result_summary)
                                .unwrap_or_else(|| "Result received".into());
                            if failed && !summary.starts_with("Failed") {
                                summary = summary
                                    .replacen("Result received", "Failed", 1)
                                    .replacen("Completed", "Failed", 1);
                            }
                            results.push((history.entries.len(), call_id.clone(), summary));
                            let mut output = p
                                .get("output")
                                .map(crate::formatting::result)
                                .unwrap_or_else(|| "(no output)".into());
                            if !call_id.is_empty() {
                                output.push_str(&format!("\n\nCALL REFERENCE\n{call_id}"));
                            }
                            ("RESULT · Unmatched result".into(), output)
                        }
                        _ => continue,
                    };
                    history.entries.push(Entry {
                        tool,
                        source: None,
                        session_id: role,
                        ts,
                        text,
                    });
                }
                "event_msg" => {
                    let role = match p["type"].as_str().unwrap_or_default() {
                        "user_message" => "USER",
                        "agent_message" => "ASSISTANT",
                        _ => continue,
                    };
                    fallback.push(Entry {
                        tool: None,
                        source: None,
                        session_id: role.into(),
                        ts,
                        text: string(p, "message"),
                    });
                }
                _ => {}
            }
        }
        progress("Pairing tool calls and results", history.entries.len())?;
        pair_tools(&mut history.entries, calls, results);
        progress("Preparing session", history.entries.len())?;
        // Older logs may only contain message events. Never double display
        // response messages and their event mirrors.
        if !has_messages {
            history.entries.extend(fallback);
            history.entries.sort_by_key(|e| e.ts);
        }
        if !model.is_empty() {
            info.push_str(&format!("\nModel: {model}"));
        }
        Ok(Self {
            history,
            info,
            id,
            path: path.to_path_buf(),
        })
    }
}

/// Pair by identity, not adjacency; keep the group at the call's position.
fn pair_tools(
    entries: &mut Vec<Entry>,
    calls: Vec<(usize, String)>,
    results: Vec<(usize, String, String)>,
) {
    use std::collections::{HashMap, HashSet, VecDeque};
    let mut pending: HashMap<String, VecDeque<usize>> = HashMap::new();
    for (index, id) in &calls {
        if !id.is_empty() {
            pending.entry(id.clone()).or_default().push_back(*index);
        }
    }
    let mut paired = HashSet::new();
    let mut removed = HashSet::new();
    for (result_index, id, summary) in results {
        if let Some(call_index) = pending.get_mut(&id).and_then(VecDeque::pop_front) {
            let failed = entries[result_index]
                .tool
                .as_ref()
                .is_some_and(|tool| tool.failed);
            if let Some(tool) = &mut entries[call_index].tool {
                tool.failed = failed;
            }
            let output = std::mem::take(&mut entries[result_index].text);
            let input = std::mem::take(&mut entries[call_index].text);
            entries[call_index].text = format!("{summary}\n\nINPUT\n{input}\n\nRESULT\n{output}");
            paired.insert(call_index);
            removed.insert(result_index);
        }
    }
    for (index, id) in calls {
        if !paired.contains(&index) {
            let input = std::mem::take(&mut entries[index].text);
            entries[index].text = format!("No result recorded\n\nINPUT\n{input}");
            if !id.is_empty() {
                entries[index]
                    .text
                    .push_str(&format!("\n\nCALL REFERENCE\n{id}"));
            }
        }
    }
    let mut index = 0;
    entries.retain(|_| {
        let keep = !removed.contains(&index);
        index += 1;
        keep
    });
}

fn render_value(value: &Value) -> String {
    match value.as_str() {
        Some(text) => crate::formatting::typeset(text),
        None => crate::formatting::render(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pairs_interleaved_calls_by_id_and_preserves_unmatched_records() {
        let payloads = [
            serde_json::json!({"type":"function_call","call_id":"a","name":"shell","arguments":"command A"}),
            serde_json::json!({"type":"custom_tool_call","call_id":"b","name":"search","input":"query B"}),
            serde_json::json!({"type":"message","role":"assistant","content":[{"text":"Working"}]}),
            serde_json::json!({"type":"custom_tool_call_output","call_id":"b","output":{"output":"answer B","exit_code":0,"wall_time_seconds":1.2}}),
            serde_json::json!({"type":"function_call_output","call_id":"a","output":{"stderr":"error A","exit_code":2}}),
            serde_json::json!({"type":"function_call","call_id":"pending","name":"shell","arguments":"waiting"}),
            serde_json::json!({"type":"function_call_output","call_id":"orphan","output":"orphan result"}),
            serde_json::json!({"type":"function_call_output","call_id":"a","output":"extra result"}),
            serde_json::json!({"type":"function_call","name":"anonymous","arguments":"no id"}),
            serde_json::json!({"type":"function_call_output","output":"no id output"}),
        ];
        let data = payloads
            .iter()
            .map(|p| serde_json::json!({"type":"response_item","payload":p}).to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let session = Session::parse(data.as_bytes(), Path::new("demo")).unwrap();
        let entries = &session.history.entries;
        assert_eq!(entries.len(), 8);
        assert!(entries[0].text.starts_with("Failed · exit 2"));
        assert_eq!(entries[0].tool.as_ref().unwrap().summary, "command A");
        assert!(entries[0].tool.as_ref().unwrap().failed);
        assert!(!entries[1].tool.as_ref().unwrap().failed);
        assert!(entries[0].text.contains("command A"));
        assert!(entries[0].text.contains("error A"));
        assert!(!entries[0].text.contains("answer B"));
        assert!(entries[1].text.starts_with("Completed · 1.2s"));
        assert!(entries[1].text.contains("query B"));
        assert!(entries[1].text.contains("answer B"));
        assert_eq!(entries[2].text, "Working");
        assert!(entries[3].text.starts_with("No result recorded"));
        assert!(entries[4].text.contains("orphan result"));
        assert!(entries[5].text.contains("extra result"));
        assert!(entries[6].text.starts_with("No result recorded"));
        assert!(entries[7].session_id.starts_with("RESULT ·"));
        let mut app = crate::app::App::from_session(session);
        app.query = "answer B".into();
        app.filter();
        assert_eq!(app.visible, vec![1]);
        assert!(app.tool_collapsed(1));
    }

    #[test]
    fn matches_a_result_recorded_before_its_call() {
        let data = r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"a","output":"answer"}}
{"type":"response_item","payload":{"type":"function_call","call_id":"a","name":"tool","arguments":"input"}}"#;
        let session = Session::parse(data.as_bytes(), Path::new("demo")).unwrap();
        assert_eq!(session.history.entries.len(), 1);
        assert_eq!(session.history.entries[0].session_id, "TOOL · tool");
        assert!(session.history.entries[0].text.contains("answer"));
    }
    #[test]
    fn finds_nested_session_by_metadata_and_rejects_missing_id() {
        let root = std::env::temp_dir().join(format!(
            "codex-session-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let nested = root.join("2026/09/16");
        fs::create_dir_all(&nested).unwrap();
        let path = nested.join("custom.jsonl");
        fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"session_id\":\"demo\"}}\n",
        )
        .unwrap();
        assert_eq!(find(&root, "demo").unwrap(), path);
        assert!(find(&root, "missing").is_err());
        assert!(find(&root, "").is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn parses_messages_tools_and_metadata_without_event_duplicates() {
        let data = r#"{"type":"session_meta","payload":{"id":"test","cwd":"/demo"}}
{"type":"turn_context","payload":{"model":"demo-model"}}
{"type":"event_msg","payload":{"type":"user_message","message":"duplicate"}}
{"type":"response_item","payload":{"type":"message","role":"developer","content":[{"text":"instructions"}]}}
{"type":"response_item","timestamp":"2026-09-16T00:00:00Z","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"你好"},{"type":"input_image"}]}}
{"type":"response_item","payload":{"type":"function_call","call_id":"1","name":"shell","arguments":"ls"}}
{"type":"response_item","payload":{"type":"function_call_output","call_id":"1","output":{"ok":true}}}
{"type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Done"}]}}
broken"#;
        let session = Session::parse(data.as_bytes(), Path::new("demo")).unwrap();
        assert_eq!(session.history.entries.len(), 3);
        assert_eq!(session.history.skipped, 1);
        assert_eq!(session.history.entries[0].text, "你好\n[input_image]");
        assert!(session.history.entries[0].ts > 0);
        assert_eq!(session.history.entries[1].session_id, "TOOL · shell");
        assert!(session.history.entries[1].text.contains("INPUT\nls"));
        assert!(
            session.history.entries[1]
                .text
                .contains("CALL REFERENCE\n1")
        );
        assert!(session.info.contains("demo-model"));
    }
    #[test]
    fn supports_event_only_logs() {
        let data = r#"{"type":"event_msg","payload":{"type":"user_message","message":"hello"}}
{"type":"event_msg","payload":{"type":"agent_message","message":"world"}}"#;
        let s = Session::parse(data.as_bytes(), Path::new("demo")).unwrap();
        assert_eq!(s.history.entries.len(), 2);
        assert_eq!(s.history.entries[1].text, "world");
    }
}
