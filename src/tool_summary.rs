//! Short display-only descriptions. Never evaluate tool inputs.
use serde_json::Value;

pub fn summarize(input: &Value) -> String {
    fn compact(text: &str) -> String {
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control() || c.is_whitespace())
            .collect();
        let text = clean.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut chars = text.chars();
        let prefix: String = chars.by_ref().take(120).collect();
        if chars.next().is_some() {
            format!("{prefix}…")
        } else {
            prefix
        }
    }
    fn value_text(value: &Value) -> Option<String> {
        match value {
            Value::String(text) if !text.trim().is_empty() => Some(compact(text)),
            Value::Array(parts) if parts.iter().all(Value::is_string) => Some(compact(
                &parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
            )),
            _ => None,
        }
    }
    fn extract(input: &Value, depth: usize) -> String {
        if depth > 8 {
            return String::new();
        }
        if let Some(text) = input.as_str() {
            if let Ok(json) = serde_json::from_str::<Value>(text) {
                return extract(&json, depth + 1);
            }
            // apply_patch input has a useful file header after its opening marker.
            for line in text.lines() {
                for marker in ["*** Update File: ", "*** Add File: ", "*** Delete File: "] {
                    if let Some(path) = line.strip_prefix(marker) {
                        return compact(path);
                    }
                }
            }
            return compact(
                text.lines()
                    .find(|line| !line.trim().is_empty())
                    .unwrap_or_default(),
            );
        }
        for key in [
            "cmd",
            "command",
            "file_path",
            "path",
            "filename",
            "file",
            "pattern",
            "query",
            "q",
            "url",
        ] {
            if let Some(value) = input.get(key).and_then(value_text)
                && !value.is_empty()
            {
                return value;
            }
        }
        for key in ["parameters", "arguments", "input"] {
            if let Some(value) = input.get(key) {
                let summary = extract(value, depth + 1);
                if !summary.is_empty() {
                    return summary;
                }
            }
        }
        String::new()
    }
    extract(input, 0)
}

/// Only explicit error metadata or recognized process envelopes imply failure.
pub fn failed(output: &Value) -> bool {
    fn inspect(value: &Value, depth: usize) -> bool {
        if depth > 16 {
            return false;
        }
        if let Some(text) = value.as_str() {
            if let Ok(decoded) = serde_json::from_str::<Value>(text) {
                return inspect(&decoded, depth + 1);
            }
            return crate::formatting::result_summary(value).starts_with("Failed");
        }
        if let Some(parts) = value.as_array() {
            return parts.iter().any(|part| inspect(part, depth + 1));
        }
        if value.get("isError").and_then(Value::as_bool) == Some(true)
            || value
                .get("exit_code")
                .and_then(Value::as_i64)
                .is_some_and(|code| code != 0)
        {
            return true;
        }
        ["content", "output", "text"].iter().any(|key| {
            value
                .get(key)
                .is_some_and(|nested| inspect(nested, depth + 1))
        })
    }
    inspect(output, 0)
}

/// Retain exact shell text or exact argv data; never reconstruct an executable
/// command from the shortened display summary.
pub fn recorded_command(input: &Value) -> Option<crate::history::RecordedCommand> {
    fn extract(input: &Value, depth: usize) -> Option<crate::history::RecordedCommand> {
        if depth > 8 {
            return None;
        }
        if let Some(text) = input.as_str() {
            return serde_json::from_str::<Value>(text)
                .ok()
                .and_then(|value| extract(&value, depth + 1));
        }
        for key in ["cmd", "command"] {
            match input.get(key) {
                Some(Value::String(text)) => {
                    return Some(crate::history::RecordedCommand::Shell(text.clone()));
                }
                Some(Value::Array(args)) if args.iter().all(Value::is_string) => {
                    return Some(crate::history::RecordedCommand::Arguments(
                        args.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect(),
                    ));
                }
                _ => {}
            }
        }
        ["parameters", "arguments", "input"]
            .iter()
            .find_map(|key| input.get(key).and_then(|value| extract(value, depth + 1)))
    }
    extract(input, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_commands_paths_queries_and_patch_files() {
        let original = "printf '%s' \"$HOME\"\necho 中文\n".repeat(20);
        assert_eq!(
            recorded_command(
                &serde_json::json!({"arguments": serde_json::json!({"cmd":original}).to_string()})
            ),
            Some(crate::history::RecordedCommand::Shell(original))
        );
        assert_eq!(
            recorded_command(&serde_json::json!({"command":["bash", "-lc", "echo 'a b'"]})),
            Some(crate::history::RecordedCommand::Arguments(vec![
                "bash".into(),
                "-lc".into(),
                "echo 'a b'".into()
            ]))
        );
        assert_eq!(
            summarize(&serde_json::json!(r#"{"cmd":"cargo test","cwd":"/work"}"#)),
            "cargo test"
        );
        assert_eq!(
            summarize(&serde_json::json!({"command":["bash","-lc","ls"]})),
            "bash -lc ls"
        );
        assert_eq!(
            summarize(&serde_json::json!({"file_path":"src/main.rs"})),
            "src/main.rs"
        );
        assert_eq!(
            summarize(&serde_json::json!({"parameters":{"query":"hello world"}})),
            "hello world"
        );
        assert_eq!(
            summarize(&serde_json::json!(
                "*** Begin Patch\n*** Update File: src/app.rs\n@@"
            )),
            "src/app.rs"
        );
        let summary = summarize(&serde_json::json!("中文".repeat(100)));
        assert!(summary.ends_with('…'));
        assert_eq!(summary.chars().count(), 121);
        assert!(summarize(&serde_json::json!({"unknown": true})).is_empty());
        assert!(recorded_command(&serde_json::json!({"file_path":"src/main.rs"})).is_none());
    }
    #[test]
    fn errors_need_metadata_not_error_words_in_output() {
        assert!(!failed(
            &serde_json::json!({"exit_code":0,"output":"Failed tests: 0"})
        ));
        assert!(failed(
            &serde_json::json!({"content":[{"type":"text","text":"{\"exit_code\":2,\"output\":\"oops\"}"}]})
        ));
        assert!(failed(&serde_json::json!({"isError":true,"content":[]})));
        assert!(failed(&serde_json::json!(
            "Chunk ID: demo\nProcess exited with code 1\nFinal output:\nerror"
        )));
        assert!(!failed(&serde_json::json!("error: documentation example")));
    }
}
