use regex::Regex;
use roxmltree::{Document, Node};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestSummary {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub skipped: u32,
    pub inconclusive: u32,
    pub duration_sec: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestFailure {
    pub name: String,
    pub full_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stack_trace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TestResults {
    pub summary: TestSummary,
    pub failures: Vec<TestFailure>,
}

pub fn parse_nunit_results(path: &Path, max_failures: usize) -> Result<TestResults, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    parse_nunit_text(&text, max_failures)
}

pub fn parse_nunit_text(text: &str, max_failures: usize) -> Result<TestResults, String> {
    let doc = Document::parse(text).map_err(|e| e.to_string())?;
    let root = doc.root_element();
    let summary_node = find_summary_node(root).unwrap_or(root);
    let summary = extract_summary(summary_node, root);
    let failures = root
        .descendants()
        .filter(|n| n.has_tag_name("test-case"))
        .filter(|n| is_failed_test_case(*n))
        .take(max_failures)
        .map(extract_failure)
        .collect();

    Ok(TestResults { summary, failures })
}

fn find_summary_node<'a, 'input>(root: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    if root.has_attribute("total") || root.has_attribute("testcasecount") || root.has_attribute("test-case-count") {
        return Some(root);
    }
    root.descendants().find(|n| n.has_attribute("total"))
}

fn extract_summary(summary_node: Node<'_, '_>, root: Node<'_, '_>) -> TestSummary {
    let total = attr_u32(summary_node, &["total", "testcasecount", "test-case-count"])
        .unwrap_or_else(|| count_cases(root));
    let failed = attr_u32(summary_node, &["failed", "failures"]).unwrap_or_else(|| {
        root.descendants()
            .filter(|n| n.has_tag_name("test-case") && is_failed_test_case(*n))
            .count() as u32
    });
    let skipped = attr_u32(summary_node, &["skipped", "ignored"]).unwrap_or_else(|| {
        root.descendants()
            .filter(|n| n.has_tag_name("test-case"))
            .filter(|n| matches!(n.attribute("result"), Some("Skipped") | Some("Ignored")))
            .count() as u32
    });
    let inconclusive = attr_u32(summary_node, &["inconclusive"]).unwrap_or_else(|| {
        root.descendants()
            .filter(|n| n.has_tag_name("test-case"))
            .filter(|n| n.attribute("result") == Some("Inconclusive"))
            .count() as u32
    });
    let passed = attr_u32(summary_node, &["passed"]).unwrap_or_else(|| {
        let computed = total.saturating_sub(failed + skipped + inconclusive);
        if total == 0 { 0 } else { computed }
    });
    let duration_sec = attr_f64(summary_node, &["duration", "durationSec", "time"]).unwrap_or(0.0);

    TestSummary {
        total,
        passed,
        failed,
        skipped,
        inconclusive,
        duration_sec,
    }
}

fn extract_failure(case: Node<'_, '_>) -> TestFailure {
    let failure = case.children().find(|n| n.has_tag_name("failure"));
    let message = failure
        .and_then(|f| child_text(f, "message"))
        .or_else(|| child_text(case, "message"))
        .map(clean_text);
    let stack_trace = failure
        .and_then(|f| child_text(f, "stack-trace"))
        .or_else(|| child_text(case, "stack-trace"))
        .map(clean_text);

    let (file, line) = extract_file_line_from_attrs(case).or_else(|| {
        stack_trace
            .as_deref()
            .and_then(extract_file_line_from_stack_trace)
    })
    .unwrap_or((None, None));

    TestFailure {
        name: case.attribute("name").unwrap_or_default().to_string(),
        full_name: case
            .attribute("fullname")
            .or_else(|| case.attribute("fullName"))
            .or_else(|| case.attribute("name"))
            .unwrap_or_default()
            .to_string(),
        result: case.attribute("result").map(ToOwned::to_owned),
        duration_sec: attr_f64(case, &["duration", "time"]),
        message,
        stack_trace,
        file,
        line,
        category: extract_category(case),
    }
}

fn extract_category(case: Node<'_, '_>) -> Option<String> {
    let categories = case
        .descendants()
        .filter(|n| n.has_tag_name("category"))
        .filter_map(|n| n.attribute("name"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if categories.is_empty() {
        None
    } else {
        Some(categories.join(";"))
    }
}

fn extract_file_line_from_attrs(case: Node<'_, '_>) -> Option<(Option<String>, Option<u32>)> {
    let file = case.attribute("file").map(ToOwned::to_owned);
    let line = case.attribute("line").and_then(|v| v.parse().ok());
    if file.is_some() || line.is_some() {
        Some((file, line))
    } else {
        None
    }
}

pub fn extract_file_line_from_stack_trace(stack: &str) -> Option<(Option<String>, Option<u32>)> {
    let patterns = [
        r"\bin\s+(.+?\.cs):line\s+(\d+)",
        r"\bin\s+(.+?\.cs):(\d+)",
        r"(.+?\.cs)\((\d+),(\d+)\)",
    ];

    for pattern in patterns {
        let re = Regex::new(pattern).expect("valid stack trace regex");
        if let Some(caps) = re.captures(stack) {
            let file = caps.get(1).map(|m| m.as_str().trim().to_string());
            let line = caps.get(2).and_then(|m| m.as_str().parse().ok());
            return Some((file, line));
        }
    }
    None
}

fn child_text(node: Node<'_, '_>, tag: &str) -> Option<String> {
    node.children()
        .find(|n| n.has_tag_name(tag))
        .and_then(|n| n.text())
        .map(ToOwned::to_owned)
}

fn clean_text(text: String) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

fn is_failed_test_case(case: Node<'_, '_>) -> bool {
    matches!(case.attribute("result"), Some("Failed") | Some("Error"))
        || case.children().any(|n| n.has_tag_name("failure"))
}

fn count_cases(root: Node<'_, '_>) -> u32 {
    root.descendants()
        .filter(|n| n.has_tag_name("test-case"))
        .count() as u32
}

fn attr_u32(node: Node<'_, '_>, names: &[&str]) -> Option<u32> {
    names.iter().find_map(|name| node.attribute(*name)?.parse().ok())
}

fn attr_f64(node: Node<'_, '_>, names: &[&str]) -> Option<f64> {
    names.iter().find_map(|name| node.attribute(*name)?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_passed_summary() {
        let xml = r#"<test-run total="1" passed="1" failed="0" skipped="0" inconclusive="0" duration="0.12"><test-suite><test-case name="A" fullname="A" result="Passed" /></test-suite></test-run>"#;
        let result = parse_nunit_text(xml, 10).unwrap();
        assert_eq!(result.summary.total, 1);
        assert!(result.failures.is_empty());
    }

    #[test]
    fn parses_failed_case_message_stack_and_file_line() {
        let xml = r#"<test-run total="1" passed="0" failed="1" skipped="0" inconclusive="0" duration="1.2"><test-case name="ShouldCreateBar" fullname="FooTests.ShouldCreateBar" result="Failed" duration="0.12"><failure><message><![CDATA[Expected: 10 But was: 9]]></message><stack-trace><![CDATA[  at FooTests.ShouldCreateBar () in Assets/Tests/FooTests.cs:line 42]]></stack-trace></failure></test-case></test-run>"#;
        let result = parse_nunit_text(xml, 10).unwrap();
        let failure = &result.failures[0];
        assert_eq!(failure.message.as_deref(), Some("Expected: 10 But was: 9"));
        assert_eq!(failure.file.as_deref(), Some("Assets/Tests/FooTests.cs"));
        assert_eq!(failure.line, Some(42));
    }

    #[test]
    fn decodes_xml_entities() {
        let xml = r#"<test-run total="1" failed="1"><test-case name="A" result="Failed"><failure><message>Expected &lt;foo&gt; &amp; got bar</message></failure></test-case></test-run>"#;
        let result = parse_nunit_text(xml, 10).unwrap();
        assert_eq!(
            result.failures[0].message.as_deref(),
            Some("Expected <foo> & got bar")
        );
    }

    #[test]
    fn supports_self_closing_test_case() {
        let xml = r#"<test-run total="1" passed="1" failed="0"><test-case name="A" result="Passed" /></test-run>"#;
        let result = parse_nunit_text(xml, 10).unwrap();
        assert_eq!(result.summary.total, 1);
        assert_eq!(result.summary.failed, 0);
    }
}
