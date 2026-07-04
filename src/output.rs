use crate::cli::{OutputFormat, TestPlatform};
use crate::log_parse::Diagnostic;
use crate::nunit::{TestFailure, TestSummary};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    TestsFailed,
    CompileError,
    UnityStartupError,
    PackageError,
    LicenseError,
    Timeout,
    ResultsMissing,
    ResultsParseError,
    UnknownError,
    RunnerConfigError,
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = match self {
            Status::Passed => "passed",
            Status::TestsFailed => "tests_failed",
            Status::CompileError => "compile_error",
            Status::UnityStartupError => "unity_startup_error",
            Status::PackageError => "package_error",
            Status::LicenseError => "license_error",
            Status::Timeout => "timeout",
            Status::ResultsMissing => "results_missing",
            Status::ResultsParseError => "results_parse_error",
            Status::UnknownError => "unknown_error",
            Status::RunnerConfigError => "runner_config_error",
        };
        f.write_str(value)
    }
}

impl Status {
    pub fn ok(self) -> bool {
        matches!(self, Status::Passed)
    }

    pub fn exit_code(self) -> i32 {
        match self {
            Status::Passed => 0,
            Status::TestsFailed => 1,
            Status::RunnerConfigError => 3,
            _ => 2,
        }
    }

    pub fn is_infra_error(self) -> bool {
        !matches!(self, Status::Passed | Status::TestsFailed | Status::RunnerConfigError)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityOutput {
    pub editor_path: Option<String>,
    pub project_version: Option<String>,
    pub resolved_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtifactsOutput {
    pub results_xml: Option<String>,
    pub log: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub log_tail: Option<String>,
    pub kept: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunOutput {
    pub schema_version: u32,
    pub ok: bool,
    pub status: Status,
    pub platform: TestPlatform,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unity: Option<UnityOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    pub summary: Option<TestSummary>,
    pub failures: Vec<TestFailure>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ArtifactsOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
    /// Present only when `platform` is `All`. Contains per-platform results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<RunOutput>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileCheckOutput {
    pub schema_version: u32,
    pub ok: bool,
    pub status: Status,
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unity: Option<UnityOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_sec: Option<f64>,
    pub diagnostics: Vec<Diagnostic>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<ArtifactsOutput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<Vec<String>>,
}

impl CompileCheckOutput {
    pub fn minimal_error(status: Status, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ok: status.ok(),
            status,
            mode: "compile_check",
            project: None,
            unity: None,
            exit_code: None,
            duration_sec: None,
            diagnostics: vec![Diagnostic::simple(status.default_diagnostic_kind(), message)],
            artifacts: None,
            dry_run: None,
            command: None,
        }
    }
}

impl RunOutput {
    pub fn minimal_error(status: Status, platform: TestPlatform, message: impl Into<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ok: status.ok(),
            status,
            platform,
            project: None,
            unity: None,
            exit_code: None,
            duration_sec: None,
            summary: None,
            failures: Vec::new(),
            diagnostics: vec![Diagnostic::simple(status.default_diagnostic_kind(), message)],
            artifacts: None,
            dry_run: None,
            command: None,
            runs: None,
        }
    }
}

impl Status {
    pub fn default_diagnostic_kind(self) -> &'static str {
        match self {
            Status::CompileError => "compile_error",
            Status::UnityStartupError => "unity_startup_error",
            Status::PackageError => "package_error",
            Status::LicenseError => "license_error",
            Status::Timeout => "timeout",
            Status::ResultsMissing => "results_missing",
            Status::ResultsParseError => "results_parse_error",
            Status::RunnerConfigError => "project_invalid",
            Status::TestsFailed => "tests_failed",
            Status::Passed => "passed",
            Status::UnknownError => "unknown_error",
        }
    }
}

pub fn serialize_output<T: Serialize>(value: &T, format: OutputFormat) -> Result<String, serde_json::Error> {
    match format {
        OutputFormat::Minimal => serialize_minimal_output(value),
        OutputFormat::CompactJson => serde_json::to_string(value),
        OutputFormat::PrettyJson => serde_json::to_string_pretty(value),
    }
}

fn serialize_minimal_output<T: Serialize>(value: &T) -> Result<String, serde_json::Error> {
    let json = serde_json::to_value(value)?;
    if json.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok("ok".to_string())
    } else {
        serde_json::to_string(value)
    }
}

pub fn path_to_json(path: &Path) -> String {
    crate::path_util::path_to_json_string(path)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionOutput {
    pub name: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    struct TestOutput {
        ok: bool,
        status: &'static str,
    }

    #[test]
    fn minimal_output_is_plain_ok_on_success() {
        let output = TestOutput {
            ok: true,
            status: "passed",
        };

        assert_eq!(serialize_output(&output, OutputFormat::Minimal).unwrap(), "ok");
    }

    #[test]
    fn minimal_output_falls_back_to_compact_json_on_failure() {
        let output = TestOutput {
            ok: false,
            status: "tests_failed",
        };

        assert_eq!(
            serialize_output(&output, OutputFormat::Minimal).unwrap(),
            r#"{"ok":false,"status":"tests_failed"}"#
        );
    }
}
