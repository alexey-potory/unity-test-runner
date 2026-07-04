mod classify;
mod cli;
mod config;
mod error;
mod log_parse;
mod nunit;
mod output;
mod path_util;
mod progress;
mod run;
mod unity;

use crate::classify::{classify, ClassificationInput};
use crate::cli::{Cli, Commands, CommonArgs, CompileCheckArgs, OutputFormat, RunArgs, TestPlatform};
use crate::config::{absolutize, apply_cli_overrides, load_config, Config};
use crate::error::RunnerError;
use crate::log_parse::{log_tail, parse_log_file, Diagnostic};
use crate::nunit::{parse_nunit_results, TestResults, TestSummary};
use crate::output::{serialize_output, path_to_json, CompileCheckOutput, RunOutput, Status, UnityOutput, VersionOutput, SCHEMA_VERSION};
use crate::progress::ProgressLogger;
use crate::run::{artifacts_output, cleanup_artifacts, compile_check_artifacts_output, ensure_artifact_dir, maybe_artifacts_output, maybe_compile_check_artifacts_output, remove_old_artifacts, resolve_artifact_paths, resolve_artifact_paths_for_label, run_unity_process, ArtifactPaths, ProcessRunResult};
use crate::unity::{build_unity_command_args, build_unity_compile_check_args, command_as_strings, resolve_editor, validate_project, ProjectInfo, UnityCommandOptions, UnityCompileCheckOptions, UnityEditorResolution};
use clap::Parser;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();
    match dispatch(cli) {
        Ok(code) => ExitCode::from(code as u8),
        Err(err) => {
            eprintln!("{err}");
            ExitCode::from(err.exit_code() as u8)
        }
    }
}

fn dispatch(cli: Cli) -> Result<i32, RunnerError> {
    match cli.command {
        Commands::Run(args) => run_command(args),
        Commands::CompileCheck(args) => compile_check_command(args),
        Commands::Doctor(args) => doctor_command(args),
        Commands::PrintConfig(args) => print_config_command(args),
        Commands::Version => version_command(),
    }
}

fn run_command(args: RunArgs) -> Result<i32, RunnerError> {
    let fallback_format = args.common.format.unwrap_or(OutputFormat::CompactJson);
    let progress = match ProgressLogger::from_common_args(&args.common) {
        Ok(v) => v,
        Err(err) => return emit_error_json(err, fallback_format, args.platform.unwrap_or_default()),
    };
    progress.step("run: loading configuration");
    let (config, project_hint) = match load_config_for_common(&args.common) {
        Ok(v) => v,
        Err(err) => return emit_error_json(err, fallback_format, args.platform.unwrap_or_default()),
    };
    let format = config.output.format;
    let requested_platform = args.platform.unwrap_or(config.tests.default_platform);

    progress.step(format!("run: validating Unity project at {}", project_hint.display()));
    let project = match validate_project(&project_hint) {
        Ok(p) => p,
        Err(err) => return emit_error_json(err, format, requested_platform),
    };

    progress.step(format!("run: resolving Unity editor for project version {}", project.editor_version));
    let editor = match resolve_editor(&config, &project) {
        Ok(e) => e,
        Err(err) => return emit_unity_resolution_error(err, format, requested_platform, &project),
    };
    progress.step(format!("run: Unity editor resolved to {}", editor.editor_path.display()));

    if requested_platform == TestPlatform::All {
        progress.step("run: platform All requested; running EditMode then PlayMode");
        let mut runs = Vec::new();
        for platform in [TestPlatform::EditMode, TestPlatform::PlayMode] {
            progress.step(format!("run: starting {platform}"));
            match run_platform(&args, &config, &project, &editor, platform, &progress) {
                Ok(output) => {
                    progress.step(format!("run: {platform} finished with status {}", output.status));
                    runs.push(output);
                }
                Err(err) => return emit_error_json(err, format, TestPlatform::All),
            }
        }
        let output = aggregate_run_outputs(&project, &editor, runs, args.dry_run);
        let exit_code = output.status.exit_code();
        progress.step(format!("run: aggregate status {}", output.status));
        print_json(&output, format)?;
        return Ok(exit_code);
    }

    let output = match run_platform(&args, &config, &project, &editor, requested_platform, &progress) {
        Ok(output) => output,
        Err(err) => return emit_error_json(err, format, requested_platform),
    };
    let exit_code = output.status.exit_code();
    progress.step(format!("run: final status {}", output.status));
    print_json(&output, format)?;
    Ok(exit_code)
}


fn compile_check_command(args: CompileCheckArgs) -> Result<i32, RunnerError> {
    let fallback_format = args.common.format.unwrap_or(OutputFormat::CompactJson);
    let progress = match ProgressLogger::from_common_args(&args.common) {
        Ok(v) => v,
        Err(err) => return emit_compile_check_error_json(err, fallback_format),
    };

    progress.step("compile-check: loading configuration");
    let (config, project_hint) = match load_config_for_common(&args.common) {
        Ok(v) => v,
        Err(err) => return emit_compile_check_error_json(err, fallback_format),
    };
    let format = config.output.format;

    progress.step(format!("compile-check: validating Unity project at {}", project_hint.display()));
    let project = match validate_project(&project_hint) {
        Ok(p) => p,
        Err(err) => return emit_compile_check_error_json(err, format),
    };

    progress.step(format!("compile-check: resolving Unity editor for project version {}", project.editor_version));
    let editor = match resolve_editor(&config, &project) {
        Ok(e) => e,
        Err(err) => return emit_compile_check_unity_resolution_error(err, format, &project),
    };
    progress.step(format!("compile-check: Unity editor resolved to {}", editor.editor_path.display()));

    progress.step("compile-check: resolving artifact paths");
    let paths = resolve_artifact_paths_for_label(&project.root, &config, "CompileCheck");
    if let Err(err) = ensure_artifact_dir(&paths) {
        return emit_compile_check_error_json(err, format);
    }
    remove_old_artifacts(&paths);
    progress.step(format!("compile-check: artifacts directory {}", paths.dir.display()));

    let options = UnityCompileCheckOptions {
        no_graphics: args.no_graphics,
        accept_apiupdate: args.accept_apiupdate,
        forget_project_path: args.forget_project_path,
        build_target: args.build_target.as_deref(),
        extra_unity_args: args.extra_unity_args.iter().map(String::as_str).collect(),
    };
    progress.step("compile-check: building Unity command line");
    let unity_args = build_unity_compile_check_args(&project.root, &paths.log, &options);

    if args.dry_run {
        progress.step("compile-check: dry-run requested; Unity will not be launched");
        let output = CompileCheckOutput {
            schema_version: SCHEMA_VERSION,
            ok: true,
            status: Status::Passed,
            mode: "compile_check",
            project: Some(path_to_json(&project.root)),
            unity: Some(unity_output(&editor)),
            exit_code: Some(0),
            duration_sec: Some(0.0),
            diagnostics: Vec::new(),
            artifacts: Some(compile_check_artifacts_output(&paths, true)),
            dry_run: Some(true),
            command: Some(command_as_strings(&editor.editor_path, &unity_args)),
        };
        print_json(&output, format)?;
        return Ok(0);
    }

    progress.step("compile-check: launching Unity and waiting for script compilation");
    let process = match run_unity_process(&editor.editor_path, &unity_args, config.runner.timeout_secs) {
        Ok(process) => process,
        Err(err) => return emit_compile_check_error_json(err, format),
    };
    progress.step(format!(
        "compile-check: Unity finished; exitCode={:?}, timedOut={}",
        process.exit_code, process.timed_out
    ));

    progress.step("compile-check: parsing Unity log diagnostics");
    let mut diagnostics = collect_compile_check_diagnostics(&paths, &config, &process);
    let mut status = classify_compile_check(&process, &diagnostics);

    if status == Status::UnknownError && !diagnostics.iter().any(|d| d.kind == "unknown_error") {
        diagnostics.push(Diagnostic::simple(
            "unknown_error",
            format!(
                "Unity exited with code {:?} while checking compilation, but no known diagnostic pattern matched.",
                process.exit_code
            ),
        ));
    }

    add_configured_tail(&mut diagnostics, &paths, &config, status);
    progress.step("compile-check: writing log tail artifact if needed");
    write_tail_artifact(&mut diagnostics, &paths);

    status = classify_compile_check(&process, &diagnostics);
    progress.step(format!("compile-check: cleaning up artifacts according to policy; status {status}"));
    let kept = cleanup_artifacts(status, &config, &paths).unwrap_or(true);

    let output = CompileCheckOutput {
        schema_version: SCHEMA_VERSION,
        ok: status.ok(),
        status,
        mode: "compile_check",
        project: Some(path_to_json(&project.root)),
        unity: Some(unity_output(&editor)),
        exit_code: process.exit_code,
        duration_sec: Some(round2(process.duration_sec)),
        diagnostics,
        artifacts: maybe_compile_check_artifacts_output(&paths, kept),
        dry_run: None,
        command: None,
    };
    progress.step(format!("compile-check: completed with status {status}"));
    print_json(&output, format)?;
    Ok(status.exit_code())
}

fn run_platform(
    args: &RunArgs,
    config: &Config,
    project: &ProjectInfo,
    editor: &UnityEditorResolution,
    platform: TestPlatform,
    progress: &ProgressLogger,
) -> Result<RunOutput, RunnerError> {
    debug_assert!(platform != TestPlatform::All, "All must be expanded before run_platform");

    progress.step(format!("{platform}: resolving artifact paths"));
    let paths = resolve_artifact_paths(&project.root, config, platform);
    ensure_artifact_dir(&paths)?;
    remove_old_artifacts(&paths);
    progress.step(format!("{platform}: artifacts directory {}", paths.dir.display()));

    let test_filter = args
        .test_filter
        .as_deref()
        .or_else(|| non_empty(&config.tests.default_filter));
    let category = args
        .category
        .as_deref()
        .or_else(|| non_empty(&config.tests.default_category));
    let command_options = UnityCommandOptions {
        test_filter,
        category,
        test_names: args.test_names.as_deref(),
        assembly_names: args.assembly_names.iter().map(String::as_str).collect(),
        assembly_type: args.assembly_type,
        requires_play_mode: args.requires_play_mode,
        run_synchronously: args.run_synchronously,
        ordered_test_list: args.ordered_test_list.as_deref(),
        test_settings: args.test_settings.as_deref(),
        player_heartbeat_timeout: args.player_heartbeat_timeout,
        build_player_path: args.build_player_path.as_deref(),
        build_target: args.build_target.as_deref(),
        no_graphics: args.no_graphics,
        accept_apiupdate: args.accept_apiupdate,
        forget_project_path: args.forget_project_path,
        extra_unity_args: args.extra_unity_args.iter().map(String::as_str).collect(),
    };
    progress.step(format!("{platform}: building Unity command line"));
    let unity_args = build_unity_command_args(
        &project.root,
        platform,
        &paths.results_xml,
        &paths.log,
        &command_options,
    );

    if args.dry_run {
        progress.step(format!("{platform}: dry-run requested; Unity will not be launched"));
        return Ok(RunOutput {
            schema_version: SCHEMA_VERSION,
            ok: true,
            status: Status::Passed,
            platform,
            project: Some(path_to_json(&project.root)),
            unity: Some(unity_output(editor)),
            exit_code: Some(0),
            duration_sec: Some(0.0),
            summary: None,
            failures: Vec::new(),
            diagnostics: Vec::new(),
            artifacts: Some(artifacts_output(&paths, true)),
            dry_run: Some(true),
            command: Some(command_as_strings(&editor.editor_path, &unity_args)),
            runs: None,
        });
    }

    progress.step(format!("{platform}: launching Unity and waiting for completion"));
    let process = run_unity_process(&editor.editor_path, &unity_args, config.runner.timeout_secs)?;
    progress.step(format!("{platform}: Unity finished; exitCode={:?}, timedOut={}", process.exit_code, process.timed_out));

    progress.step(format!("{platform}: parsing NUnit XML results"));
    let (results, parse_error) = parse_results_if_present(&paths, config);
    let results_exists = paths.results_xml.is_file();
    progress.step(format!("{platform}: parsing Unity log diagnostics"));
    let mut diagnostics = collect_diagnostics(&paths, config, &process, results_exists, parse_error.as_deref());

    progress.step(format!("{platform}: classifying result"));
    let mut status = classify(ClassificationInput {
        runner_error: false,
        timeout: process.timed_out,
        diagnostics: &diagnostics,
        results: results.as_ref(),
        results_xml_exists: results_exists,
        results_parse_error: parse_error.is_some(),
    });

    if status == Status::ResultsMissing && !diagnostics.iter().any(|d| d.kind == "results_missing") {
        diagnostics.push(Diagnostic::simple(
            "results_missing",
            format!("Unity did not produce test results XML: {}", paths.results_xml.display()),
        ));
    }
    if status == Status::ResultsParseError && !diagnostics.iter().any(|d| d.kind == "results_parse_error") {
        diagnostics.push(Diagnostic::simple(
            "results_parse_error",
            parse_error.unwrap_or_else(|| "NUnit XML could not be parsed".to_string()),
        ));
    }
    add_configured_tail(&mut diagnostics, &paths, config, status);
    progress.step(format!("{platform}: writing log tail artifact if needed"));
    write_tail_artifact(&mut diagnostics, &paths);

    // Re-classify after adding parse/missing diagnostics; compile/license/package priority remains intact.
    status = classify(ClassificationInput {
        runner_error: false,
        timeout: process.timed_out,
        diagnostics: &diagnostics,
        results: results.as_ref(),
        results_xml_exists: results_exists,
        results_parse_error: status == Status::ResultsParseError,
    });

    progress.step(format!("{platform}: cleaning up artifacts according to policy"));
    let kept = cleanup_artifacts(status, config, &paths).unwrap_or(true);
    progress.step(format!("{platform}: completed with status {status}"));
    let (summary, failures) = match results {
        Some(r) => (Some(r.summary), r.failures),
        None => (None, Vec::new()),
    };

    Ok(RunOutput {
        schema_version: SCHEMA_VERSION,
        ok: status.ok(),
        status,
        platform,
        project: Some(path_to_json(&project.root)),
        unity: Some(unity_output(editor)),
        exit_code: process.exit_code,
        duration_sec: Some(round2(process.duration_sec)),
        summary,
        failures,
        diagnostics,
        artifacts: maybe_artifacts_output(&paths, kept),
        dry_run: None,
        command: None,
        runs: None,
    })
}

fn aggregate_run_outputs(
    project: &ProjectInfo,
    editor: &UnityEditorResolution,
    runs: Vec<RunOutput>,
    dry_run: bool,
) -> RunOutput {
    let status = aggregate_status(&runs);
    let summary = aggregate_summary(&runs);
    let failures = runs
        .iter()
        .flat_map(|run| run.failures.iter().cloned())
        .collect::<Vec<_>>();
    let diagnostics = runs
        .iter()
        .flat_map(|run| run.diagnostics.iter().cloned())
        .collect::<Vec<_>>();
    let duration_sec = if dry_run {
        Some(0.0)
    } else {
        Some(round2(runs.iter().filter_map(|run| run.duration_sec).sum()))
    };

    RunOutput {
        schema_version: SCHEMA_VERSION,
        ok: status.ok(),
        status,
        platform: TestPlatform::All,
        project: Some(path_to_json(&project.root)),
        unity: Some(unity_output(editor)),
        exit_code: Some(status.exit_code()),
        duration_sec,
        summary,
        failures,
        diagnostics,
        artifacts: None,
        dry_run: dry_run.then_some(true),
        command: None,
        runs: Some(runs),
    }
}

fn aggregate_summary(runs: &[RunOutput]) -> Option<TestSummary> {
    let mut any = false;
    let mut summary = TestSummary::default();
    for run in runs.iter().filter_map(|run| run.summary.as_ref()) {
        any = true;
        summary.total += run.total;
        summary.passed += run.passed;
        summary.failed += run.failed;
        summary.skipped += run.skipped;
        summary.inconclusive += run.inconclusive;
        summary.duration_sec += run.duration_sec;
    }
    if any {
        summary.duration_sec = round2(summary.duration_sec);
        Some(summary)
    } else {
        None
    }
}

fn aggregate_status(runs: &[RunOutput]) -> Status {
    if runs.iter().all(|run| run.status == Status::Passed) {
        return Status::Passed;
    }

    // Across two platform runs, infrastructure problems should stay actionable even if
    // the other platform produced ordinary test failures.
    const PRIORITY: &[Status] = &[
        Status::RunnerConfigError,
        Status::Timeout,
        Status::CompileError,
        Status::LicenseError,
        Status::PackageError,
        Status::UnityStartupError,
        Status::ResultsParseError,
        Status::ResultsMissing,
        Status::UnknownError,
        Status::TestsFailed,
    ];

    PRIORITY
        .iter()
        .copied()
        .find(|status| runs.iter().any(|run| run.status == *status))
        .unwrap_or(Status::UnknownError)
}

fn doctor_command(args: CommonArgs) -> Result<i32, RunnerError> {
    let fallback_format = args.format.unwrap_or(OutputFormat::CompactJson);
    let progress = match ProgressLogger::from_common_args(&args) {
        Ok(v) => v,
        Err(err) => return emit_error_json(err, fallback_format, TestPlatform::EditMode),
    };
    progress.step("doctor: loading configuration");
    let (config, project_hint) = match load_config_for_common(&args) {
        Ok(v) => v,
        Err(err) => return emit_error_json(err, fallback_format, TestPlatform::EditMode),
    };
    let format = config.output.format;
    let platform = config.tests.default_platform;
    progress.step(format!("doctor: validating Unity project at {}", project_hint.display()));
    let project = match validate_project(&project_hint) {
        Ok(p) => p,
        Err(err) => return emit_error_json(err, format, platform),
    };
    progress.step(format!("doctor: resolving Unity editor for project version {}", project.editor_version));
    let editor = match resolve_editor(&config, &project) {
        Ok(e) => e,
        Err(err) => return emit_unity_resolution_error(err, format, platform, &project),
    };
    progress.step(format!("doctor: Unity editor resolved to {}", editor.editor_path.display()));
    let paths = resolve_artifact_paths(&project.root, &config, platform);
    progress.step(format!("doctor: checking artifact directory {}", paths.dir.display()));
    let mut diagnostics = Vec::new();
    if let Err(err) = ensure_artifact_dir(&paths) {
        diagnostics.push(Diagnostic::simple(
            "runner_config_error",
            format!("cannot create artifact directory {}: {err}", paths.dir.display()),
        ));
    }
    let status = if diagnostics.is_empty() {
        Status::Passed
    } else {
        Status::RunnerConfigError
    };
    let output = RunOutput {
        schema_version: SCHEMA_VERSION,
        ok: status.ok(),
        status,
        platform,
        project: Some(path_to_json(&project.root)),
        unity: Some(unity_output(&editor)),
        exit_code: Some(status.exit_code()),
        duration_sec: None,
        summary: None,
        failures: Vec::new(),
        diagnostics,
        artifacts: Some(artifacts_output(&paths, true)),
        dry_run: None,
        command: None,
        runs: None,
    };
    progress.step(format!("doctor: completed with status {status}"));
    print_json(&output, format)?;
    Ok(status.exit_code())
}

fn print_config_command(args: CommonArgs) -> Result<i32, RunnerError> {
    let progress = ProgressLogger::from_common_args(&args)?;
    progress.step("print-config: loading and resolving configuration");
    let (config, _) = load_config_for_common(&args)?;
    let text = toml::to_string_pretty(&config).map_err(|e| RunnerError::Config(e.to_string()))?;
    progress.step("print-config: completed");
    println!("{text}");
    Ok(0)
}

fn version_command() -> Result<i32, RunnerError> {
    let output = VersionOutput {
        name: "unity-test-runner",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: SCHEMA_VERSION,
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(0)
}

fn load_config_for_common(args: &CommonArgs) -> Result<(Config, PathBuf), RunnerError> {
    let project_hint = absolutize(&args.project)?;
    let runner_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf));
    let (mut config, _) = load_config(&project_hint, args.config.as_deref(), runner_dir.as_deref())?;
    apply_cli_overrides(
        &mut config,
        args.editor.as_ref(),
        &args.editor_base,
        args.format,
        args.keep,
        args.timeout,
        args.log_tail,
        args.artifact_dir.as_ref(),
    );
    Ok((config, project_hint))
}

fn parse_results_if_present(paths: &ArtifactPaths, config: &Config) -> (Option<TestResults>, Option<String>) {
    if paths.results_xml.is_file() {
        match parse_nunit_results(&paths.results_xml, config.diagnostics.max_failures) {
            Ok(r) => (Some(r), None),
            Err(err) => (None, Some(err)),
        }
    } else {
        (None, None)
    }
}

fn collect_diagnostics(
    paths: &ArtifactPaths,
    config: &Config,
    process: &ProcessRunResult,
    results_exists: bool,
    parse_error: Option<&str>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if process.timed_out {
        diagnostics.push(Diagnostic::simple(
            "timeout",
            format!("Unity process exceeded timeout of {} seconds and was terminated.", config.runner.timeout_secs),
        ));
    }

    if paths.log.is_file() {
        let include_fallback_tail = !results_exists || parse_error.is_some() || process.exit_code.unwrap_or(0) != 0;
        diagnostics.extend(parse_log_file(
            &paths.log,
            &config.diagnostics,
            &config.output,
            include_fallback_tail,
        ));
    }

    if let Some(err) = parse_error {
        diagnostics.push(Diagnostic::simple("results_parse_error", err.to_string()));
    }

    diagnostics
}


fn collect_compile_check_diagnostics(
    paths: &ArtifactPaths,
    config: &Config,
    process: &ProcessRunResult,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if process.timed_out {
        diagnostics.push(Diagnostic::simple(
            "timeout",
            format!(
                "Unity process exceeded timeout of {} seconds and was terminated while checking compilation.",
                config.runner.timeout_secs
            ),
        ));
    }

    if paths.log.is_file() {
        let include_fallback_tail = process.timed_out || process.exit_code.unwrap_or(0) != 0;
        diagnostics.extend(parse_log_file(
            &paths.log,
            &config.diagnostics,
            &config.output,
            include_fallback_tail,
        ));
    } else if process.exit_code.unwrap_or(0) != 0 || process.timed_out {
        diagnostics.push(Diagnostic::simple(
            "unity_startup_error",
            format!("Unity did not produce a compile-check log: {}", paths.log.display()),
        ));
    }

    diagnostics
}

fn classify_compile_check(process: &ProcessRunResult, diagnostics: &[Diagnostic]) -> Status {
    if process.timed_out || has_diag(diagnostics, "timeout") {
        return Status::Timeout;
    }
    if has_diag(diagnostics, "compile_error") {
        return Status::CompileError;
    }
    if has_diag(diagnostics, "license_error") {
        return Status::LicenseError;
    }
    if has_diag(diagnostics, "package_error") {
        return Status::PackageError;
    }
    if has_diag(diagnostics, "unity_startup_error") || has_diag(diagnostics, "unity_editor_not_found") {
        return Status::UnityStartupError;
    }
    if process.exit_code.unwrap_or(1) == 0 {
        return Status::Passed;
    }
    Status::UnknownError
}

fn has_diag(diagnostics: &[Diagnostic], kind: &str) -> bool {
    diagnostics.iter().any(|d| d.kind == kind)
}

fn add_configured_tail(
    diagnostics: &mut Vec<Diagnostic>,
    paths: &ArtifactPaths,
    config: &Config,
    status: Status,
) {
    let include = match status {
        Status::Passed => config.output.include_log_tail_on_success,
        Status::TestsFailed => config.output.include_log_tail_on_test_failure,
        s if s.is_infra_error() => config.output.include_log_tail_on_infra_error,
        _ => false,
    };
    if !include || !paths.log.is_file() || diagnostics.iter().any(|d| d.kind == "log_tail") {
        return;
    }
    if let Ok(text) = fs::read_to_string(&paths.log) {
        diagnostics.push(Diagnostic {
            kind: "log_tail".to_string(),
            message: "Unity log tail requested by output settings.".to_string(),
            tail: Some(log_tail(&text, config.output.log_tail_lines)),
            ..Diagnostic::default()
        });
    }
}

fn write_tail_artifact(diagnostics: &mut [Diagnostic], paths: &ArtifactPaths) {
    let Some(diagnostic) = diagnostics.iter_mut().find(|d| d.kind == "log_tail" && d.tail.is_some()) else {
        return;
    };

    let Some(tail) = diagnostic.tail.take() else {
        return;
    };

    if let Some(parent) = paths.log_tail.parent() {
        let _ = fs::create_dir_all(parent);
    }

    match fs::write(&paths.log_tail, tail) {
        Ok(()) => {
            diagnostic.message = "Unity log tail was saved to an artifact file.".to_string();
            diagnostic.tail_path = Some(path_to_json(&paths.log_tail));
        }
        Err(err) => {
            diagnostic.message = format!(
                "Unity log tail was requested, but runner could not write tail artifact: {err}"
            );
        }
    }
}


fn emit_compile_check_error_json(err: RunnerError, format: OutputFormat) -> Result<i32, RunnerError> {
    let status = err.status();
    let output = CompileCheckOutput::minimal_error(status, err.to_string());
    print_json(&output, format)?;
    Ok(status.exit_code())
}

fn emit_compile_check_unity_resolution_error(
    err: RunnerError,
    format: OutputFormat,
    project: &ProjectInfo,
) -> Result<i32, RunnerError> {
    let message = err.to_string();
    let kind = if message.contains("unity_editor_not_found") {
        "unity_editor_not_found"
    } else {
        "unity_startup_error"
    };
    let output = CompileCheckOutput {
        schema_version: SCHEMA_VERSION,
        ok: false,
        status: Status::UnityStartupError,
        mode: "compile_check",
        project: Some(path_to_json(&project.root)),
        unity: Some(UnityOutput {
            editor_path: None,
            project_version: Some(project.editor_version.clone()),
            resolved_version: None,
        }),
        exit_code: None,
        duration_sec: None,
        diagnostics: vec![Diagnostic::simple(kind, message)],
        artifacts: None,
        dry_run: None,
        command: None,
    };
    print_json(&output, format)?;
    Ok(Status::UnityStartupError.exit_code())
}

fn emit_error_json(err: RunnerError, format: OutputFormat, platform: TestPlatform) -> Result<i32, RunnerError> {
    let status = err.status();
    let output = RunOutput::minimal_error(status, platform, err.to_string());
    print_json(&output, format)?;
    Ok(status.exit_code())
}

fn emit_unity_resolution_error(
    err: RunnerError,
    format: OutputFormat,
    platform: TestPlatform,
    project: &ProjectInfo,
) -> Result<i32, RunnerError> {
    let message = err.to_string();
    let kind = if message.contains("unity_editor_not_found") {
        "unity_editor_not_found"
    } else {
        "unity_startup_error"
    };
    let output = RunOutput {
        schema_version: SCHEMA_VERSION,
        ok: false,
        status: Status::UnityStartupError,
        platform,
        project: Some(path_to_json(&project.root)),
        unity: Some(UnityOutput {
            editor_path: None,
            project_version: Some(project.editor_version.clone()),
            resolved_version: None,
        }),
        exit_code: None,
        duration_sec: None,
        summary: None,
        failures: Vec::new(),
        diagnostics: vec![Diagnostic::simple(kind, message)],
        artifacts: None,
        dry_run: None,
        command: None,
        runs: None,
    };
    print_json(&output, format)?;
    Ok(Status::UnityStartupError.exit_code())
}

fn unity_output(editor: &UnityEditorResolution) -> UnityOutput {
    UnityOutput {
        editor_path: Some(path_to_json(&editor.editor_path)),
        project_version: Some(editor.project_version.clone()),
        resolved_version: Some(editor.resolved_version.clone()),
    }
}

fn print_json<T: serde::Serialize>(output: &T, format: OutputFormat) -> Result<(), RunnerError> {
    println!("{}", serialize_output(output, format)?);
    Ok(())
}

fn non_empty(s: &str) -> Option<&str> {
    if s.trim().is_empty() {
        None
    } else {
        Some(s)
    }
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
