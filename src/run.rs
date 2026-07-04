use crate::cli::TestPlatform;
use crate::config::Config;
use crate::error::RunnerError;
use crate::output::{path_to_json, ArtifactsOutput, Status};
use crate::path_util::normalize_for_unity;
use crate::unity::join_relative_path;
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;
use wait_timeout::ChildExt;

#[derive(Debug, Clone)]
pub struct ArtifactPaths {
    pub dir: PathBuf,
    pub results_xml: PathBuf,
    pub log: PathBuf,
    pub log_tail: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ProcessRunResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub duration_sec: f64,
}

pub fn resolve_artifact_paths(
    project_root: &Path,
    config: &Config,
    platform: TestPlatform,
) -> ArtifactPaths {
    resolve_artifact_paths_for_label(project_root, config, &platform.to_string())
}

pub fn resolve_artifact_paths_for_label(
    project_root: &Path,
    config: &Config,
    platform: &str,
) -> ArtifactPaths {
    let artifact_dir = resolve_artifact_dir(project_root, &config.paths.artifact_dir, platform);
    let results_name = config
        .paths
        .results_file_name
        .replace("{platform}", platform);
    let log_name = config.paths.log_file_name.replace("{platform}", platform);
    let log_tail_name = config
        .paths
        .log_tail_file_name
        .replace("{platform}", platform);
    ArtifactPaths {
        dir: normalize_for_unity(artifact_dir.clone()),
        results_xml: normalize_for_unity(artifact_dir.join(results_name)),
        log: normalize_for_unity(artifact_dir.join(log_name)),
        log_tail: normalize_for_unity(artifact_dir.join(log_tail_name)),
    }
}

pub fn ensure_artifact_dir(paths: &ArtifactPaths) -> Result<(), RunnerError> {
    fs::create_dir_all(&paths.dir)?;
    Ok(())
}

pub fn remove_old_artifacts(paths: &ArtifactPaths) {
    let _ = fs::remove_file(&paths.results_xml);
    let _ = fs::remove_file(&paths.log);
    let _ = fs::remove_file(&paths.log_tail);
}

pub fn run_unity_process(
    editor: &Path,
    args: &[OsString],
    timeout_secs: u64,
) -> Result<ProcessRunResult, RunnerError> {
    let started = Instant::now();
    let mut child = Command::new(editor)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;

    let timeout = std::time::Duration::from_secs(timeout_secs);
    let status = match child.wait_timeout(timeout)? {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(ProcessRunResult {
                exit_code: None,
                timed_out: true,
                duration_sec: started.elapsed().as_secs_f64(),
            });
        }
    };

    Ok(ProcessRunResult {
        exit_code: status.code(),
        timed_out: false,
        duration_sec: started.elapsed().as_secs_f64(),
    })
}

pub fn cleanup_artifacts(
    status: Status,
    config: &Config,
    paths: &ArtifactPaths,
) -> Result<bool, RunnerError> {
    let keep = config.runner.keep_artifacts
        || match status {
            Status::Passed => !config.runner.cleanup_success,
            Status::TestsFailed => !config.runner.cleanup_failed_tests,
            Status::RunnerConfigError => true,
            _ => !config.runner.cleanup_infra_errors,
        };

    if !keep {
        if paths.results_xml.exists() {
            fs::remove_file(&paths.results_xml)?;
        }
        if paths.log.exists() {
            fs::remove_file(&paths.log)?;
        }
        if paths.log_tail.exists() {
            fs::remove_file(&paths.log_tail)?;
        }
    }
    Ok(keep)
}

pub fn artifacts_output(paths: &ArtifactPaths, kept: bool) -> ArtifactsOutput {
    ArtifactsOutput {
        results_xml: Some(path_to_json(&paths.results_xml)),
        log: Some(path_to_json(&paths.log)),
        log_tail: if paths.log_tail.exists() {
            Some(path_to_json(&paths.log_tail))
        } else {
            None
        },
        kept,
    }
}

pub fn compile_check_artifacts_output(paths: &ArtifactPaths, kept: bool) -> ArtifactsOutput {
    ArtifactsOutput {
        results_xml: None,
        log: Some(path_to_json(&paths.log)),
        log_tail: if paths.log_tail.exists() {
            Some(path_to_json(&paths.log_tail))
        } else {
            None
        },
        kept,
    }
}

fn resolve_artifact_dir(project_root: &Path, configured: &str, platform: &str) -> PathBuf {
    let configured = configured.trim();
    let template = if configured.is_empty()
        || configured.eq_ignore_ascii_case("temp")
        || configured.eq_ignore_ascii_case("system_temp")
        || configured.eq_ignore_ascii_case("system-temp")
    {
        r"{temp}/unity-test-runner/{project}-{project_hash}".to_string()
    } else {
        configured.to_string()
    };

    let expanded = crate::config::expand_env_vars(&template);
    let temp_dir = normalize_for_unity(std::env::temp_dir());
    let project_name = project_slug(project_root);
    let project_hash = short_project_hash(project_root);

    let expanded = expanded
        .replace("{temp}", &temp_dir.to_string_lossy())
        .replace("{system_temp}", &temp_dir.to_string_lossy())
        .replace("{system-temp}", &temp_dir.to_string_lossy())
        .replace("{project}", &project_name)
        .replace("{project_hash}", &project_hash)
        .replace("{project-hash}", &project_hash)
        .replace("{platform}", platform);

    let path = PathBuf::from(&expanded);
    if path.is_absolute() {
        normalize_for_unity(path)
    } else {
        normalize_for_unity(join_relative_path(project_root, &expanded))
    }
}

fn project_slug(project_root: &Path) -> String {
    let raw = project_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("project");
    let slug = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

fn short_project_hash(project_root: &Path) -> String {
    let normalized = normalize_for_unity(project_root.to_path_buf())
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    let mut hasher = DefaultHasher::new();
    normalized.hash(&mut hasher);
    format!("{:08x}", hasher.finish() as u32)
}


pub fn maybe_artifacts_output(paths: &ArtifactPaths, kept: bool) -> Option<ArtifactsOutput> {
    if kept || paths.results_xml.exists() || paths.log.exists() || paths.log_tail.exists() {
        Some(artifacts_output(paths, kept))
    } else {
        None
    }
}

pub fn maybe_compile_check_artifacts_output(paths: &ArtifactPaths, kept: bool) -> Option<ArtifactsOutput> {
    if kept || paths.log.exists() || paths.log_tail.exists() {
        Some(compile_check_artifacts_output(paths, kept))
    } else {
        None
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_artifact_dir_uses_system_temp() {
        let config = Config::default();
        let paths = resolve_artifact_paths(Path::new("/repo/My Game"), &config, TestPlatform::EditMode);
        assert!(paths.dir.starts_with(std::env::temp_dir()));
        assert!(paths.dir.to_string_lossy().contains("unity-test-runner"));
        assert!(paths.dir.to_string_lossy().contains("My_Game"));
    }

    #[test]
    fn relative_artifact_dir_stays_project_relative() {
        let mut config = Config::default();
        config.paths.artifact_dir = r".codex\unity-test-results".to_string();
        let paths = resolve_artifact_paths(Path::new("/repo/Game"), &config, TestPlatform::PlayMode);
        assert_eq!(paths.dir, Path::new("/repo/Game/.codex/unity-test-results"));
        assert!(paths.results_xml.to_string_lossy().contains("UnityTestResults-PlayMode.xml"));
    }
}
