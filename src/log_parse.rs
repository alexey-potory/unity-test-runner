use crate::config::{DiagnosticsConfig, OutputConfig};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tail_path: Option<String>,
}

impl Diagnostic {
    pub fn simple(kind: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            message: message.into(),
            ..Self::default()
        }
    }
}

pub fn parse_log_file(
    path: &Path,
    diagnostics_cfg: &DiagnosticsConfig,
    output_cfg: &OutputConfig,
    include_fallback_tail: bool,
) -> Vec<Diagnostic> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_log_text(&text, diagnostics_cfg, output_cfg, include_fallback_tail)
}

pub fn parse_log_text(
    text: &str,
    diagnostics_cfg: &DiagnosticsConfig,
    output_cfg: &OutputConfig,
    include_fallback_tail: bool,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    diagnostics.extend(extract_compile_errors(text, diagnostics_cfg.max_compile_errors));

    if let Some(diagnostic) = extract_license_error(text) {
        diagnostics.push(diagnostic);
    }
    if let Some(diagnostic) = extract_package_error(text) {
        diagnostics.push(diagnostic);
    }
    if let Some(diagnostic) = extract_startup_error(text) {
        diagnostics.push(diagnostic);
    }
    diagnostics.extend(extract_exception_blocks(text, diagnostics_cfg));

    if include_fallback_tail && diagnostics.is_empty() {
        diagnostics.push(Diagnostic {
            kind: "log_tail".to_string(),
            message: "Unity failed before producing test results; no known error pattern matched.".to_string(),
            tail: Some(log_tail(text, output_cfg.log_tail_lines)),
            ..Diagnostic::default()
        });
    }

    diagnostics
}

fn extract_compile_errors(text: &str, limit: usize) -> Vec<Diagnostic> {
    let re = Regex::new(
        r"(?m)^(.+?\.cs)\((\d+),(\d+)\):\s+error\s+(CS\d+):\s+(.+)$",
    )
    .expect("valid compile error regex");

    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for caps in re.captures_iter(text) {
        let message = caps
            .get(0)
            .map(|m| m.as_str().trim().to_string())
            .unwrap_or_default();
        let file = caps.get(1).map(|m| m.as_str().to_string());
        let line = caps.get(2).and_then(|m| m.as_str().parse().ok());
        let column = caps.get(3).and_then(|m| m.as_str().parse().ok());
        let code = caps.get(4).map(|m| m.as_str().to_string());

        let key = format!(
            "{}|{:?}|{:?}|{}|{}",
            file.as_deref()
                .unwrap_or_default()
                .replace('\\', "/")
                .to_ascii_lowercase(),
            line,
            column,
            code.as_deref().unwrap_or_default(),
            message.replace('\\', "/")
        );

        if !seen.insert(key) {
            continue;
        }

        out.push(Diagnostic {
            kind: "compile_error".to_string(),
            message,
            file,
            line,
            column,
            code,
            ..Diagnostic::default()
        });

        if out.len() >= limit {
            break;
        }
    }

    out
}

fn extract_license_error(text: &str) -> Option<Diagnostic> {
    let patterns = [
        "no valid unity editor license",
        "license is invalid",
        "licensing client failed",
        "license activation failed",
        "license system",
    ];
    find_first_line(text, |line_lower| {
        patterns.iter().any(|p| line_lower.contains(p))
    })
    .map(|line| Diagnostic::simple("license_error", line))
}

fn extract_package_error(text: &str) -> Option<Diagnostic> {
    find_first_line(text, |line_lower| {
        line_lower.contains("failed to resolve packages")
            || line_lower.contains("package manager") && line_lower.contains("error")
            || line_lower.contains("upm") && line_lower.contains("error")
            || line_lower.contains("package resolution failed")
    })
    .map(|line| Diagnostic::simple("package_error", line))
}

fn extract_startup_error(text: &str) -> Option<Diagnostic> {
    find_first_line(text, |line_lower| {
        line_lower.contains("failed to load project")
            || line_lower.contains("could not open project")
            || line_lower.contains("aborting batchmode due to failure")
            || line_lower.contains("project path does not exist")
            || line_lower.contains("unity editor failed to start")
    })
    .map(|line| Diagnostic::simple("unity_startup_error", line))
}

fn find_first_line<F>(text: &str, mut predicate: F) -> Option<String>
where
    F: FnMut(&str) -> bool,
{
    text.lines().find_map(|line| {
        let lower = line.to_ascii_lowercase();
        if predicate(&lower) {
            Some(line.trim().to_string())
        } else {
            None
        }
    })
}

fn extract_exception_blocks(text: &str, cfg: &DiagnosticsConfig) -> Vec<Diagnostic> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let exception_re = Regex::new(
        r"(?i)(unhandled exception|\b[a-z0-9_.]+exception:|\bexception:)"
    )
    .expect("valid exception regex");

    let mut i = 0;
    while i < lines.len() && out.len() < cfg.max_exception_blocks {
        let line = lines[i];
        if exception_re.is_match(line) && !line.to_ascii_lowercase().contains("compilation") {
            let start = i.saturating_sub(cfg.context_lines);
            let mut end = i + 1;
            let mut stack_lines = 0usize;
            while end < lines.len() {
                let next = lines[end];
                let looks_like_stack = next.trim_start().starts_with("at ")
                    || next.trim_start().starts_with("---")
                    || next.contains(" in ") && next.contains(".cs:");
                if looks_like_stack && stack_lines < cfg.stack_trace_lines {
                    stack_lines += 1;
                    end += 1;
                    continue;
                }
                if end <= i + cfg.context_lines {
                    end += 1;
                    continue;
                }
                break;
            }
            out.push(Diagnostic {
                kind: "exception".to_string(),
                message: line.trim().to_string(),
                stack_trace: Some(lines[start..end].join("\n")),
                ..Diagnostic::default()
            });
            i = end;
        } else {
            i += 1;
        }
    }
    out
}

pub fn log_tail(text: &str, lines: usize) -> String {
    let mut tail: Vec<&str> = text.lines().rev().take(lines).collect();
    tail.reverse();
    tail.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn cfg() -> Config {
        Config::default()
    }

    #[test]
    fn extracts_compile_error() {
        let text = "Assets/Scripts/Foo.cs(17,22): error CS0103: The name 'bar' does not exist\n";
        let parsed = parse_log_text(text, &cfg().diagnostics, &cfg().output, false);
        assert_eq!(parsed[0].kind, "compile_error");
        assert_eq!(parsed[0].file.as_deref(), Some("Assets/Scripts/Foo.cs"));
        assert_eq!(parsed[0].line, Some(17));
        assert_eq!(parsed[0].column, Some(22));
        assert_eq!(parsed[0].code.as_deref(), Some("CS0103"));
    }

    #[test]
    fn deduplicates_repeated_compile_errors() {
        let text = "Assets/Scripts/Foo.cs(17,22): error CS0103: The name 'bar' does not exist\n\
Assets/Scripts/Foo.cs(17,22): error CS0103: The name 'bar' does not exist\n\
Assets\\Scripts\\Foo.cs(17,22): error CS0103: The name 'bar' does not exist\n";
        let parsed = parse_log_text(text, &cfg().diagnostics, &cfg().output, false);
        let compile_errors: Vec<_> = parsed.iter().filter(|d| d.kind == "compile_error").collect();
        assert_eq!(compile_errors.len(), 1);
    }

    #[test]
    fn extracts_license_error() {
        let text = "No valid Unity Editor license found. Please activate.\n";
        let parsed = parse_log_text(text, &cfg().diagnostics, &cfg().output, false);
        assert_eq!(parsed[0].kind, "license_error");
    }

    #[test]
    fn extracts_package_error() {
        let text = "Package Manager: Error failed to resolve packages\n";
        let parsed = parse_log_text(text, &cfg().diagnostics, &cfg().output, false);
        assert_eq!(parsed[0].kind, "package_error");
    }

    #[test]
    fn extracts_exception_block() {
        let text = "Before\nUnhandled exception: System.Exception: boom\n  at Foo.Bar () in Assets/Foo.cs:line 1\nAfter\n";
        let parsed = parse_log_text(text, &cfg().diagnostics, &cfg().output, false);
        assert!(parsed.iter().any(|d| d.kind == "exception"));
    }

    #[test]
    fn returns_fallback_tail() {
        let text = "a\nb\nc\n";
        let mut config = cfg();
        config.output.log_tail_lines = 2;
        let parsed = parse_log_text(text, &config.diagnostics, &config.output, true);
        assert_eq!(parsed[0].kind, "log_tail");
        assert_eq!(parsed[0].tail.as_deref(), Some("b\nc"));
    }
}
