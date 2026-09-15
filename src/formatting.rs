//! Human-readable JSON fields, decoded strings, and indented list items.
use serde_json::Value;

pub fn typeset(text: &str) -> String {
    match serde_json::from_str::<Value>(text) {
        Ok(value @ (Value::Object(_) | Value::Array(_))) => render(&value),
        _ => text.to_owned(),
    }
}

pub fn render(value: &Value) -> String {
    let mut lines = Vec::new();
    visit(value, 0, 0, &mut lines);
    lines.join("\n")
}

fn visit(value: &Value, indent: usize, depth: usize, lines: &mut Vec<String>) {
    let pad = " ".repeat(indent);
    match value {
        Value::Object(fields) if !fields.is_empty() => {
            for (key, value) in fields {
                match value {
                    Value::Bool(_) | Value::Number(_) | Value::Null => {
                        lines.push(format!("{pad}{key}: {value}"))
                    }
                    _ => {
                        lines.push(format!("{pad}{key}:"));
                        visit(value, indent + 2, depth + 1, lines);
                    }
                }
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, value) in items.iter().enumerate() {
                lines.push(format!("{pad}• Item {}", index + 1));
                visit(value, indent + 2, depth + 1, lines);
            }
        }
        Value::Object(_) => lines.push(format!("{pad}(empty object)")),
        Value::Array(_) => lines.push(format!("{pad}(empty list)")),
        Value::String(text) => {
            // Some tool outputs contain another JSON document inside a string.
            if depth < 16
                && let Ok(nested @ (Value::Object(_) | Value::Array(_))) =
                    serde_json::from_str::<Value>(text)
            {
                visit(&nested, indent, depth + 1, lines);
                return;
            }
            for line in text.split('\n') {
                lines.push(format!("{pad}{line}"));
            }
        }
        _ => lines.push(format!("{pad}{value}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typesets_fields_arrays_and_escaped_multiline_strings() {
        let text = typeset(
            r#"{"command":"echo hello\necho world","items":[{"ok":true}],"output":"{\"count\":2}"}"#,
        );
        assert!(text.contains("command:\n  echo hello\n  echo world"));
        assert!(text.contains("• Item 1\n    ok: true"));
        assert!(text.contains("output:\n  count: 2"));
        assert!(!text.contains('{'));
    }
    #[test]
    fn preserves_plain_text_and_handles_empty_values() {
        assert_eq!(
            typeset("# Hello\n```rust\nfn main() {}\n```"),
            "# Hello\n```rust\nfn main() {}\n```"
        );
        assert_eq!(typeset("{incomplete"), "{incomplete");
        assert_eq!(typeset("[]"), "(empty list)");
    }
}

/// Prefer the actual tool output over transport fields and content wrappers.
pub fn result(value: &Value) -> String {
    result_inner(value, 0)
}

fn result_inner(value: &Value, depth: usize) -> String {
    if depth > 16 {
        return render(value);
    }
    if let Some(text) = value.as_str() {
        if let Ok(decoded @ (Value::Object(_) | Value::Array(_))) =
            serde_json::from_str::<Value>(text)
        {
            return result_inner(&decoded, depth + 1);
        }
        // Shell tools may return a textual envelope instead of JSON.
        if let Some((header, output)) = text.split_once("Final output:\n")
            && (header.starts_with("Chunk ID:") || header.starts_with("Wall time:"))
        {
            return format!(
                "OUTPUT\n{}\n\nDETAILS\n{}",
                if output.trim().is_empty() {
                    "(no output)"
                } else {
                    output
                },
                header.trim()
            );
        }
        return if text.trim().is_empty() {
            "(no output)".into()
        } else {
            text.to_owned()
        };
    }
    if let Some(parts) = value.as_array() {
        return parts
            .iter()
            .map(|part| result_inner(part, depth + 1))
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    let Some(fields) = value.as_object() else {
        return render(value);
    };
    let mut sections = Vec::new();
    if let Some(code) = fields.get("exit_code").filter(|code| !code.is_null()) {
        sections.push(format!(
            "{} · exit {code}",
            if code.as_i64() == Some(0) {
                "COMPLETED"
            } else {
                "FAILED"
            }
        ));
    } else if fields.get("isError").and_then(Value::as_bool) == Some(true) {
        sections.push("FAILED".into());
    }
    let mut remaining = fields.clone();
    remaining.remove("exit_code");
    if fields.get("isError").and_then(Value::as_bool).is_some() {
        remaining.remove("isError");
    }
    let mut found_output = false;
    for (key, label) in [
        ("stdout", "OUTPUT"),
        ("output", "OUTPUT"),
        ("stderr", "ERROR OUTPUT"),
        ("content", "OUTPUT"),
        ("text", "OUTPUT"),
    ] {
        if let Some(body) = remaining.remove(key) {
            found_output = true;
            if body.as_str().is_some_and(|s| s.trim().is_empty()) && key == "stderr" {
                continue;
            }
            sections.push(format!("{label}\n{}", result_inner(&body, depth + 1)));
        }
    }
    if !found_output {
        return render(value);
    }
    if remaining.get("type").and_then(Value::as_str) == Some("text") {
        remaining.remove("type");
    }
    if !remaining.is_empty() {
        sections.push(format!("DETAILS\n{}", render(&Value::Object(remaining))));
    }
    // Content blocks often nest the same Output heading; flatten that wrapper.
    sections
        .join("\n\n")
        .replace("OUTPUT\nOUTPUT\n", "OUTPUT\n")
}

#[cfg(test)]
mod result_tests {
    use super::*;
    #[test]
    fn unwraps_content_and_prioritizes_output_over_metadata() {
        let value = serde_json::json!({"exit_code":0,"wall_time_seconds":1.2,"output":"{\"content\":[{\"type\":\"text\",\"text\":\"hello\\nworld\"}]}"});
        let text = result(&value);
        assert!(text.starts_with("COMPLETED · exit 0\n\nOUTPUT\nhello\nworld"));
        assert!(text.find("hello").unwrap() < text.find("wall_time_seconds").unwrap());
        assert!(!text.contains("Item 1"));
        assert!(!text.contains("type:"));
    }
    #[test]
    fn preserves_errors_and_unknown_metadata() {
        let text = result(
            &serde_json::json!({"exit_code":2,"stdout":"","stderr":"permission denied","extra":{"trace":"abc"}}),
        );
        assert!(text.starts_with("FAILED · exit 2"));
        assert!(text.contains("ERROR OUTPUT\npermission denied"));
        assert!(text.contains("trace:\n    abc"));
    }
}

/// Only report success/failure or duration when explicitly recorded by the tool.
pub fn result_summary(value: &Value) -> String {
    fn inspect(value: &Value, depth: usize) -> (Option<String>, Option<f64>) {
        if depth > 16 {
            return (None, None);
        }
        if let Some(text) = value.as_str() {
            if let Ok(decoded) = serde_json::from_str::<Value>(text) {
                return inspect(&decoded, depth + 1);
            }
            let header = text.split_once("Final output:\n").map(|(header, _)| header);
            if let Some(header) =
                header.filter(|h| h.starts_with("Chunk ID:") || h.starts_with("Wall time:"))
            {
                let code = header.lines().find_map(|line| {
                    line.strip_prefix("Process exited with code ")
                        .and_then(|s| s.trim().parse::<i64>().ok())
                });
                let seconds = header.lines().find_map(|line| {
                    line.strip_prefix("Wall time: ")
                        .and_then(|s| s.split_whitespace().next())
                        .and_then(|s| s.parse::<f64>().ok())
                });
                return (
                    code.map(|code| {
                        if code == 0 {
                            "Completed".into()
                        } else {
                            format!("Failed · exit {code}")
                        }
                    }),
                    seconds,
                );
            }
        }
        let status = if value["isError"].as_bool() == Some(true) {
            Some("Failed".into())
        } else {
            value["exit_code"].as_i64().map(|code| {
                if code == 0 {
                    "Completed".into()
                } else {
                    format!("Failed · exit {code}")
                }
            })
        };
        let seconds = value["wall_time_seconds"]
            .as_f64()
            .or_else(|| value["wall_time"].as_f64());
        (status, seconds)
    }
    let (status, seconds) = inspect(value, 0);
    let mut summary = status.unwrap_or_else(|| "Result received".into());
    if let Some(seconds) = seconds.filter(|s| s.is_finite() && *s >= 0.0) {
        summary.push_str(&format!(" · {seconds:.1}s"));
    }
    summary
}
