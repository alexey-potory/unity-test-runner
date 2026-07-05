use crate::cli::{OutputFormat, TestPlatform};
use crate::error::RunnerError;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub schema_version: u32,
    pub unity: UnityConfig,
    pub tests: TestsConfig,
    pub paths: PathsConfig,
    pub runner: RunnerConfig,
    pub output: OutputConfig,
    pub diagnostics: DiagnosticsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnityConfig {
    pub search_roots: Vec<String>,
    pub prefer_project_version: bool,
    pub require_exact_project_version: bool,
    pub fallback_policy: FallbackPolicy,
    pub editor_executable: String,
    pub windows_executable_relative_path: String,
    pub macos_executable_relative_path: String,
    pub linux_executable_relative_path: String,
}

impl UnityConfig {
    pub(crate) fn executable_relative_path(&self) -> &str {
        self.executable_relative_path_for(env::consts::OS)
    }

    fn executable_relative_path_for(&self, os: &str) -> &str {
        match os {
            "macos" => &self.macos_executable_relative_path,
            "linux" => &self.linux_executable_relative_path,
            _ => &self.windows_executable_relative_path,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FallbackPolicy {
    None,
    LatestInstalled,
    SameMajorMinor,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestsConfig {
    pub default_platform: TestPlatform,
    pub default_filter: String,
    pub default_category: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathsConfig {
    pub artifact_dir: String,
    pub results_file_name: String,
    pub log_file_name: String,
    pub log_tail_file_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunnerConfig {
    pub timeout_secs: u64,
    pub keep_artifacts: bool,
    pub cleanup_success: bool,
    pub cleanup_failed_tests: bool,
    pub cleanup_infra_errors: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputConfig {
    pub format: OutputFormat,
    pub include_passed_tests: bool,
    pub include_log_tail_on_success: bool,
    pub include_log_tail_on_test_failure: bool,
    pub include_log_tail_on_infra_error: bool,
    pub log_tail_lines: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticsConfig {
    pub max_failures: usize,
    pub max_compile_errors: usize,
    pub max_exception_blocks: usize,
    pub stack_trace_lines: usize,
    pub context_lines: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: 1,
            unity: UnityConfig {
                search_roots: vec![
                    r"C:\Program Files\Unity\Hub\Editor".to_string(),
                    r"%LOCALAPPDATA%\Unity\Hub\Editor".to_string(),
                    "/Applications/Unity/Hub/Editor".to_string(),
                    "$HOME/Unity/Hub/Editor".to_string(),
                ],
                prefer_project_version: true,
                require_exact_project_version: true,
                fallback_policy: FallbackPolicy::None,
                editor_executable: String::new(),
                windows_executable_relative_path: r"Editor\Unity.exe".to_string(),
                macos_executable_relative_path: "Unity.app/Contents/MacOS/Unity".to_string(),
                linux_executable_relative_path: "Editor/Unity".to_string(),
            },
            tests: TestsConfig {
                default_platform: TestPlatform::EditMode,
                default_filter: String::new(),
                default_category: String::new(),
            },
            paths: PathsConfig {
                artifact_dir: r"{temp}/unity-test-runner/{project}-{project_hash}".to_string(),
                results_file_name: "UnityTestResults-{platform}.xml".to_string(),
                log_file_name: "UnityTest-{platform}.log".to_string(),
                log_tail_file_name: "UnityTestTail-{platform}.txt".to_string(),
            },
            runner: RunnerConfig {
                timeout_secs: 900,
                keep_artifacts: false,
                cleanup_success: true,
                cleanup_failed_tests: false,
                cleanup_infra_errors: false,
            },
            output: OutputConfig {
                format: OutputFormat::CompactJson,
                include_passed_tests: false,
                include_log_tail_on_success: false,
                include_log_tail_on_test_failure: false,
                include_log_tail_on_infra_error: true,
                log_tail_lines: 80,
            },
            diagnostics: DiagnosticsConfig {
                max_failures: 10,
                max_compile_errors: 20,
                max_exception_blocks: 5,
                stack_trace_lines: 12,
                context_lines: 4,
            },
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialConfig {
    schema_version: Option<u32>,
    unity: Option<PartialUnityConfig>,
    tests: Option<PartialTestsConfig>,
    paths: Option<PartialPathsConfig>,
    runner: Option<PartialRunnerConfig>,
    output: Option<PartialOutputConfig>,
    diagnostics: Option<PartialDiagnosticsConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialUnityConfig {
    search_roots: Option<Vec<String>>,
    prefer_project_version: Option<bool>,
    require_exact_project_version: Option<bool>,
    fallback_policy: Option<FallbackPolicy>,
    editor_executable: Option<String>,
    windows_executable_relative_path: Option<String>,
    macos_executable_relative_path: Option<String>,
    linux_executable_relative_path: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialTestsConfig {
    default_platform: Option<TestPlatform>,
    default_filter: Option<String>,
    default_category: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialPathsConfig {
    artifact_dir: Option<String>,
    results_file_name: Option<String>,
    log_file_name: Option<String>,
    log_tail_file_name: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialRunnerConfig {
    timeout_secs: Option<u64>,
    keep_artifacts: Option<bool>,
    cleanup_success: Option<bool>,
    cleanup_failed_tests: Option<bool>,
    cleanup_infra_errors: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialOutputConfig {
    format: Option<OutputFormat>,
    include_passed_tests: Option<bool>,
    include_log_tail_on_success: Option<bool>,
    include_log_tail_on_test_failure: Option<bool>,
    include_log_tail_on_infra_error: Option<bool>,
    log_tail_lines: Option<usize>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct PartialDiagnosticsConfig {
    max_failures: Option<usize>,
    max_compile_errors: Option<usize>,
    max_exception_blocks: Option<usize>,
    stack_trace_lines: Option<usize>,
    context_lines: Option<usize>,
}

impl PartialConfig {
    fn apply_to(self, target: &mut Config) {
        if let Some(v) = self.schema_version {
            target.schema_version = v;
        }
        if let Some(u) = self.unity {
            if let Some(v) = u.search_roots {
                target.unity.search_roots = v;
            }
            if let Some(v) = u.prefer_project_version {
                target.unity.prefer_project_version = v;
            }
            if let Some(v) = u.require_exact_project_version {
                target.unity.require_exact_project_version = v;
            }
            if let Some(v) = u.fallback_policy {
                target.unity.fallback_policy = v;
            }
            if let Some(v) = u.editor_executable {
                target.unity.editor_executable = v;
            }
            if let Some(v) = u.windows_executable_relative_path {
                target.unity.windows_executable_relative_path = v;
            }
            if let Some(v) = u.macos_executable_relative_path {
                target.unity.macos_executable_relative_path = v;
            }
            if let Some(v) = u.linux_executable_relative_path {
                target.unity.linux_executable_relative_path = v;
            }
        }
        if let Some(t) = self.tests {
            if let Some(v) = t.default_platform {
                target.tests.default_platform = v;
            }
            if let Some(v) = t.default_filter {
                target.tests.default_filter = v;
            }
            if let Some(v) = t.default_category {
                target.tests.default_category = v;
            }
        }
        if let Some(p) = self.paths {
            if let Some(v) = p.artifact_dir {
                target.paths.artifact_dir = v;
            }
            if let Some(v) = p.results_file_name {
                target.paths.results_file_name = v;
            }
            if let Some(v) = p.log_file_name {
                target.paths.log_file_name = v;
            }
            if let Some(v) = p.log_tail_file_name {
                target.paths.log_tail_file_name = v;
            }
        }
        if let Some(r) = self.runner {
            if let Some(v) = r.timeout_secs {
                target.runner.timeout_secs = v;
            }
            if let Some(v) = r.keep_artifacts {
                target.runner.keep_artifacts = v;
            }
            if let Some(v) = r.cleanup_success {
                target.runner.cleanup_success = v;
            }
            if let Some(v) = r.cleanup_failed_tests {
                target.runner.cleanup_failed_tests = v;
            }
            if let Some(v) = r.cleanup_infra_errors {
                target.runner.cleanup_infra_errors = v;
            }
        }
        if let Some(o) = self.output {
            if let Some(v) = o.format {
                target.output.format = v;
            }
            if let Some(v) = o.include_passed_tests {
                target.output.include_passed_tests = v;
            }
            if let Some(v) = o.include_log_tail_on_success {
                target.output.include_log_tail_on_success = v;
            }
            if let Some(v) = o.include_log_tail_on_test_failure {
                target.output.include_log_tail_on_test_failure = v;
            }
            if let Some(v) = o.include_log_tail_on_infra_error {
                target.output.include_log_tail_on_infra_error = v;
            }
            if let Some(v) = o.log_tail_lines {
                target.output.log_tail_lines = v;
            }
        }
        if let Some(d) = self.diagnostics {
            if let Some(v) = d.max_failures {
                target.diagnostics.max_failures = v;
            }
            if let Some(v) = d.max_compile_errors {
                target.diagnostics.max_compile_errors = v;
            }
            if let Some(v) = d.max_exception_blocks {
                target.diagnostics.max_exception_blocks = v;
            }
            if let Some(v) = d.stack_trace_lines {
                target.diagnostics.stack_trace_lines = v;
            }
            if let Some(v) = d.context_lines {
                target.diagnostics.context_lines = v;
            }
        }
    }
}

pub fn load_config(
    project_root: &Path,
    explicit_config: Option<&Path>,
    runner_dir: Option<&Path>,
) -> Result<(Config, Vec<PathBuf>), RunnerError> {
    let mut config = Config::default();
    let mut loaded = Vec::new();

    if let Some(dir) = runner_dir {
        let skill_default = dir.join("..").join("config").join("default.toml");
        apply_if_exists(&skill_default, &mut config, &mut loaded)?;
    }

    let project_config = project_root.join("unity-test-runner.toml");
    apply_if_exists(&project_config, &mut config, &mut loaded)?;

    let codex_config = project_root.join(".codex").join("unity-test-runner.toml");
    apply_if_exists(&codex_config, &mut config, &mut loaded)?;

    if let Some(path) = explicit_config {
        let path = absolutize(path)?;
        if !path.is_file() {
            return Err(RunnerError::Config(format!(
                "explicit config file does not exist: {}",
                path.display()
            )));
        }
        apply_config_file(&path, &mut config)?;
        loaded.push(path);
    }

    Ok((config, loaded))
}

fn apply_if_exists(
    path: &Path,
    config: &mut Config,
    loaded: &mut Vec<PathBuf>,
) -> Result<(), RunnerError> {
    if path.is_file() {
        apply_config_file(path, config)?;
        loaded.push(path.to_path_buf());
    }
    Ok(())
}

fn apply_config_file(path: &Path, config: &mut Config) -> Result<(), RunnerError> {
    let text = fs::read_to_string(path)?;
    let partial: PartialConfig = toml::from_str(&text).map_err(|e| RunnerError::Toml {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    partial.apply_to(config);
    Ok(())
}

pub fn apply_cli_overrides(
    config: &mut Config,
    editor: Option<&PathBuf>,
    editor_base: &[PathBuf],
    format: Option<OutputFormat>,
    keep: bool,
    timeout: Option<u64>,
    log_tail: Option<usize>,
    artifact_dir: Option<&PathBuf>,
) {
    if let Some(path) = editor {
        config.unity.editor_executable = path.to_string_lossy().to_string();
    }
    if !editor_base.is_empty() {
        let mut roots: Vec<String> = editor_base
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        roots.extend(config.unity.search_roots.clone());
        config.unity.search_roots = roots;
    }
    if let Some(v) = format {
        config.output.format = v;
    }
    if keep {
        config.runner.keep_artifacts = true;
    }
    if let Some(v) = timeout {
        config.runner.timeout_secs = v;
    }
    if let Some(v) = log_tail {
        config.output.log_tail_lines = v;
    }
    if let Some(path) = artifact_dir {
        config.paths.artifact_dir = path.to_string_lossy().to_string();
    }
}

pub fn expanded_search_roots(config: &Config) -> Vec<PathBuf> {
    config
        .unity
        .search_roots
        .iter()
        .map(|s| PathBuf::from(expand_env_vars(s)))
        .collect()
}

pub fn expand_env_vars(input: &str) -> String {
    let percent_re = Regex::new(r"%([A-Za-z_][A-Za-z0-9_]*)%").expect("valid regex");
    let dollar_re = Regex::new(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)\}?").expect("valid regex");

    let with_percent = percent_re.replace_all(input, |caps: &regex::Captures<'_>| {
        env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
    });
    dollar_re
        .replace_all(&with_percent, |caps: &regex::Captures<'_>| {
            env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string()
}

pub fn absolutize(path: &Path) -> Result<PathBuf, RunnerError> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(env::current_dir()?.join(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_expansion_supports_percent_and_home() {
        env::set_var("UNITY_TEST_RUNNER_TMP", "X:/Tmp");
        env::set_var("HOME", "/home/example");
        assert_eq!(
            expand_env_vars(r"%UNITY_TEST_RUNNER_TMP%\Unity\$HOME"),
            r"X:/Tmp\Unity\/home/example"
        );
    }

    #[test]
    fn partial_config_overrides_defaults() {
        let mut cfg = Config::default();
        let partial: PartialConfig = toml::from_str(
            r#"
            [runner]
            timeout_secs = 10
            keep_artifacts = true
            [tests]
            default_platform = "PlayMode"
            "#,
        )
        .unwrap();
        partial.apply_to(&mut cfg);
        assert_eq!(cfg.runner.timeout_secs, 10);
        assert!(cfg.runner.keep_artifacts);
        assert_eq!(cfg.tests.default_platform, TestPlatform::PlayMode);
    }

    #[test]
    fn selects_editor_executable_for_each_supported_os() {
        let cfg = Config::default();
        assert_eq!(
            cfg.unity.executable_relative_path_for("windows"),
            r"Editor\Unity.exe"
        );
        assert_eq!(
            cfg.unity.executable_relative_path_for("macos"),
            "Unity.app/Contents/MacOS/Unity"
        );
        assert_eq!(cfg.unity.executable_relative_path_for("linux"), "Editor/Unity");
    }

    #[test]
    fn default_search_roots_cover_unity_hub_platforms() {
        let cfg = Config::default();
        assert!(cfg
            .unity
            .search_roots
            .contains(&r"C:\Program Files\Unity\Hub\Editor".to_string()));
        assert!(cfg
            .unity
            .search_roots
            .contains(&"/Applications/Unity/Hub/Editor".to_string()));
        assert!(cfg
            .unity
            .search_roots
            .contains(&"$HOME/Unity/Hub/Editor".to_string()));
    }
}
