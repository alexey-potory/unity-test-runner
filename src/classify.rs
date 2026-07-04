use crate::log_parse::Diagnostic;
use crate::nunit::TestResults;
use crate::output::Status;

#[derive(Debug, Clone, Default)]
pub struct ClassificationInput<'a> {
    pub runner_error: bool,
    pub timeout: bool,
    pub diagnostics: &'a [Diagnostic],
    pub results: Option<&'a TestResults>,
    pub results_xml_exists: bool,
    pub results_parse_error: bool,
}

pub fn classify(input: ClassificationInput<'_>) -> Status {
    if input.runner_error {
        return Status::RunnerConfigError;
    }
    if input.timeout {
        return Status::Timeout;
    }
    if has_diag(input.diagnostics, "compile_error") {
        return Status::CompileError;
    }
    if has_diag(input.diagnostics, "license_error") {
        return Status::LicenseError;
    }
    if has_diag(input.diagnostics, "package_error") {
        return Status::PackageError;
    }
    if let Some(results) = input.results {
        if results.summary.failed > 0 {
            return Status::TestsFailed;
        }
        return Status::Passed;
    }
    if input.results_parse_error {
        return Status::ResultsParseError;
    }
    if !input.results_xml_exists {
        if has_diag(input.diagnostics, "unity_startup_error")
            || has_diag(input.diagnostics, "unity_editor_not_found")
        {
            return Status::UnityStartupError;
        }
        return Status::ResultsMissing;
    }
    Status::UnknownError
}

fn has_diag(diagnostics: &[Diagnostic], kind: &str) -> bool {
    diagnostics.iter().any(|d| d.kind == kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nunit::{TestResults, TestSummary};

    #[test]
    fn compile_error_has_priority_over_failed_xml() {
        let results = TestResults {
            summary: TestSummary {
                total: 1,
                failed: 1,
                ..TestSummary::default()
            },
            failures: Vec::new(),
        };
        let diags = vec![Diagnostic::simple("compile_error", "boom")];
        assert_eq!(
            classify(ClassificationInput {
                diagnostics: &diags,
                results: Some(&results),
                results_xml_exists: true,
                ..Default::default()
            }),
            Status::CompileError
        );
    }

    #[test]
    fn failed_xml_maps_to_tests_failed() {
        let results = TestResults {
            summary: TestSummary {
                total: 1,
                failed: 1,
                ..TestSummary::default()
            },
            failures: Vec::new(),
        };
        assert_eq!(
            classify(ClassificationInput {
                results: Some(&results),
                results_xml_exists: true,
                ..Default::default()
            }),
            Status::TestsFailed
        );
    }

    #[test]
    fn timeout_has_priority() {
        let diags = vec![Diagnostic::simple("compile_error", "boom")];
        assert_eq!(
            classify(ClassificationInput {
                timeout: true,
                diagnostics: &diags,
                ..Default::default()
            }),
            Status::Timeout
        );
    }
}
