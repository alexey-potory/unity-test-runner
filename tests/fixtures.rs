use unity_test_runner::classify::{classify, ClassificationInput};
use unity_test_runner::config::Config;
use unity_test_runner::log_parse::parse_log_text;
use unity_test_runner::nunit::parse_nunit_text;
use unity_test_runner::output::Status;

#[test]
fn fixture_nunit_passed() {
    let xml = include_str!("fixtures/nunit-passed.xml");
    let results = parse_nunit_text(xml, 10).unwrap();
    assert_eq!(results.summary.total, 2);
    assert_eq!(results.summary.failed, 0);
}

#[test]
fn fixture_nunit_failed() {
    let xml = include_str!("fixtures/nunit-failed.xml");
    let results = parse_nunit_text(xml, 10).unwrap();
    assert_eq!(results.summary.failed, 1);
    assert_eq!(results.failures[0].line, Some(42));
    assert_eq!(results.failures[0].category.as_deref(), Some("Smoke"));
}

#[test]
fn fixture_log_compile_error_classifies_first() {
    let cfg = Config::default();
    let text = include_str!("fixtures/unity-compile-error.log");
    let diags = parse_log_text(text, &cfg.diagnostics, &cfg.output, false);
    assert_eq!(diags[0].kind, "compile_error");
    assert_eq!(
        classify(ClassificationInput {
            diagnostics: &diags,
            results_xml_exists: false,
            ..Default::default()
        }),
        Status::CompileError
    );
}

#[test]
fn fixture_log_license_error() {
    let cfg = Config::default();
    let text = include_str!("fixtures/unity-license-error.log");
    let diags = parse_log_text(text, &cfg.diagnostics, &cfg.output, false);
    assert_eq!(diags[0].kind, "license_error");
}

#[test]
fn fixture_log_package_error() {
    let cfg = Config::default();
    let text = include_str!("fixtures/unity-package-error.log");
    let diags = parse_log_text(text, &cfg.diagnostics, &cfg.output, false);
    assert_eq!(diags[0].kind, "package_error");
}

#[test]
fn fixture_log_exception() {
    let cfg = Config::default();
    let text = include_str!("fixtures/unity-exception-before-results.log");
    let diags = parse_log_text(text, &cfg.diagnostics, &cfg.output, false);
    assert!(diags.iter().any(|d| d.kind == "exception"));
}
