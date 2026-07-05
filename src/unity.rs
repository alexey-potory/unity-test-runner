use crate::cli::{AssemblyType, TestPlatform};
use crate::config::{expanded_search_roots, Config, FallbackPolicy};
use crate::error::RunnerError;
use crate::path_util::normalize_for_unity;
use regex::Regex;
use serde::Serialize;
use std::cmp::Ordering;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProjectInfo {
    pub root: PathBuf,
    pub editor_version: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnityEditorResolution {
    pub editor_path: PathBuf,
    pub project_version: String,
    pub resolved_version: String,
}

pub fn validate_project(project_path: &Path) -> Result<ProjectInfo, RunnerError> {
    let root = if project_path.exists() {
        normalize_for_unity(fs::canonicalize(project_path)?)
    } else {
        return Err(RunnerError::Config(format!(
            "project path does not exist: {}",
            project_path.display()
        )));
    };

    if !root.join("Assets").is_dir() {
        return Err(RunnerError::Config(format!(
            "invalid Unity project: missing Assets directory under {}",
            root.display()
        )));
    }

    let project_version_path = root.join("ProjectSettings").join("ProjectVersion.txt");
    if !project_version_path.is_file() {
        return Err(RunnerError::Config(format!(
            "invalid Unity project: missing {}",
            project_version_path.display()
        )));
    }

    let text = fs::read_to_string(&project_version_path)?;
    let version = parse_project_version(&text).ok_or_else(|| {
        RunnerError::Config(format!(
            "could not find m_EditorVersion in {}",
            project_version_path.display()
        ))
    })?;

    Ok(ProjectInfo {
        root,
        editor_version: version,
    })
}

pub fn parse_project_version(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("m_EditorVersion:")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
    })
}

pub fn resolve_editor(
    config: &Config,
    project: &ProjectInfo,
) -> Result<UnityEditorResolution, RunnerError> {
    let executable_rel = config.unity.executable_relative_path();

    if !config.unity.editor_executable.trim().is_empty() {
        let path = normalize_for_unity(PathBuf::from(crate::config::expand_env_vars(
            &config.unity.editor_executable,
        )));
        if path.is_file() {
            let resolved_version = infer_version_from_editor_path(&path)
                .unwrap_or_else(|| project.editor_version.clone());
            return Ok(UnityEditorResolution {
                editor_path: path,
                project_version: project.editor_version.clone(),
                resolved_version,
            });
        }
        return Err(RunnerError::UnityStartup(format!(
            "explicit Unity editor executable does not exist: {}",
            path.display()
        )));
    }

    let roots = expanded_search_roots(config);
    if config.unity.prefer_project_version {
        for root in &roots {
            let candidate = join_relative_path(&root.join(&project.editor_version), executable_rel);
            if candidate.is_file() {
                return Ok(UnityEditorResolution {
                    editor_path: candidate,
                    project_version: project.editor_version.clone(),
                    resolved_version: project.editor_version.clone(),
                });
            }
        }
    }

    if config.unity.require_exact_project_version && config.unity.prefer_project_version {
        return Err(RunnerError::UnityStartup(format!(
            "unity_editor_not_found: exact Unity version {} was not found in search roots: {}",
            project.editor_version,
            roots
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join("; ")
        )));
    }

    match config.unity.fallback_policy {
        FallbackPolicy::None => Err(RunnerError::UnityStartup(format!(
            "unity_editor_not_found: Unity version {} was not found and fallback_policy=none",
            project.editor_version
        ))),
        FallbackPolicy::LatestInstalled => {
            let mut resolved =
                find_latest_installed(&roots, executable_rel, None).ok_or_else(|| {
                    RunnerError::UnityStartup(format!(
                        "unity_editor_not_found: no Unity editors found in search roots: {}",
                        roots
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join("; ")
                    ))
                })?;
            resolved.project_version = project.editor_version.clone();
            Ok(resolved)
        }
        FallbackPolicy::SameMajorMinor => {
            let prefix =
                UnityVersion::parse(&project.editor_version).map(|v| (v.parts[0], v.parts[1]));
            let mut resolved =
                find_latest_installed(&roots, executable_rel, prefix).ok_or_else(|| {
                    RunnerError::UnityStartup(format!(
                        "unity_editor_not_found: no Unity editor matching major/minor of {} found",
                        project.editor_version
                    ))
                })?;
            resolved.project_version = project.editor_version.clone();
            Ok(resolved)
        }
    }
}

fn find_latest_installed(
    roots: &[PathBuf],
    executable_rel: &str,
    major_minor: Option<(u32, u32)>,
) -> Option<UnityEditorResolution> {
    let mut candidates: Vec<(UnityVersion, PathBuf, String)> = Vec::new();

    for root in roots {
        let Ok(entries) = fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let Some(version_name) = path
                .file_name()
                .and_then(|s| s.to_str())
                .map(ToOwned::to_owned)
            else {
                continue;
            };
            let Some(version) = UnityVersion::parse(&version_name) else {
                continue;
            };
            if let Some((major, minor)) = major_minor {
                if version.parts[0] != major || version.parts[1] != minor {
                    continue;
                }
            }
            let exe = join_relative_path(&path, executable_rel);
            if exe.is_file() {
                candidates.push((version, exe, version_name));
            }
        }
    }

    candidates.sort_by(|a, b| a.0.cmp(&b.0));
    candidates
        .pop()
        .map(|(_, path, version)| UnityEditorResolution {
            editor_path: path,
            project_version: String::new(),
            resolved_version: version,
        })
}

pub fn join_relative_path(base: &Path, rel: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for part in rel.split(['\\', '/']).filter(|p| !p.is_empty()) {
        out.push(part);
    }
    out
}

#[derive(Debug, Clone, Default)]
pub struct UnityCommandOptions<'a> {
    pub test_filter: Option<&'a str>,
    pub category: Option<&'a str>,
    pub test_names: Option<&'a str>,
    pub assembly_names: Vec<&'a str>,
    pub assembly_type: Option<AssemblyType>,
    pub requires_play_mode: Option<bool>,
    pub run_synchronously: bool,
    pub ordered_test_list: Option<&'a Path>,
    pub test_settings: Option<&'a Path>,
    pub player_heartbeat_timeout: Option<u64>,
    pub build_player_path: Option<&'a Path>,
    pub build_target: Option<&'a str>,
    pub no_graphics: bool,
    pub accept_apiupdate: bool,
    pub forget_project_path: bool,
    pub extra_unity_args: Vec<&'a str>,
}

pub fn build_unity_command_args(
    project: &Path,
    platform: TestPlatform,
    results_xml: &Path,
    log_file: &Path,
    options: &UnityCommandOptions<'_>,
) -> Vec<OsString> {
    let mut args = Vec::new();

    if options.accept_apiupdate {
        args.push(OsString::from("-accept-apiupdate"));
    }
    if options.no_graphics {
        args.push(OsString::from("-nographics"));
    }
    if options.forget_project_path {
        args.push(OsString::from("-forgetProjectPath"));
    }
    if let Some(build_target) = options.build_target.filter(|s| !s.trim().is_empty()) {
        args.push(OsString::from("-buildTarget"));
        args.push(OsString::from(build_target));
    }

    args.extend([
        OsString::from("-runTests"),
        OsString::from("-batchmode"),
        OsString::from("-projectPath"),
        normalize_for_unity(project).as_os_str().to_os_string(),
        OsString::from("-testPlatform"),
        OsString::from(platform.to_string()),
        OsString::from("-testResults"),
        normalize_for_unity(results_xml).as_os_str().to_os_string(),
        OsString::from("-logFile"),
        normalize_for_unity(log_file).as_os_str().to_os_string(),
    ]);

    if let Some(filter) = options.test_filter.filter(|s| !s.trim().is_empty()) {
        args.push(OsString::from("-testFilter"));
        args.push(OsString::from(filter));
    }
    if let Some(category) = options.category.filter(|s| !s.trim().is_empty()) {
        args.push(OsString::from("-testCategory"));
        args.push(OsString::from(category));
    }
    if let Some(test_names) = options.test_names.filter(|s| !s.trim().is_empty()) {
        args.push(OsString::from("-testNames"));
        args.push(OsString::from(test_names));
    }
    if !options.assembly_names.is_empty() {
        args.push(OsString::from("-assemblyNames"));
        args.push(OsString::from(options.assembly_names.join(";")));
    }
    if let Some(assembly_type) = options.assembly_type {
        args.push(OsString::from("-assemblyType"));
        args.push(OsString::from(assembly_type.to_string()));
    }
    if let Some(requires_play_mode) = options.requires_play_mode {
        args.push(OsString::from("-requiresPlayMode"));
        args.push(OsString::from(if requires_play_mode {
            "true"
        } else {
            "false"
        }));
    }
    if options.run_synchronously && platform == TestPlatform::EditMode {
        args.push(OsString::from("-runSynchronously"));
    }
    if let Some(path) = options.ordered_test_list {
        args.push(OsString::from("-orderedTestListFile"));
        args.push(normalize_for_unity(path).as_os_str().to_os_string());
    }
    if let Some(path) = options.test_settings {
        args.push(OsString::from("-testSettingsFile"));
        args.push(normalize_for_unity(path).as_os_str().to_os_string());
    }
    if let Some(seconds) = options.player_heartbeat_timeout {
        args.push(OsString::from("-playerHeartbeatTimeout"));
        args.push(OsString::from(seconds.to_string()));
    }
    if let Some(path) = options.build_player_path {
        args.push(OsString::from("-buildPlayerPath"));
        args.push(normalize_for_unity(path).as_os_str().to_os_string());
    }
    for arg in &options.extra_unity_args {
        if !arg.trim().is_empty() {
            args.push(OsString::from(arg));
        }
    }
    args
}

#[derive(Debug, Clone, Default)]
pub struct UnityCompileCheckOptions<'a> {
    pub no_graphics: bool,
    pub accept_apiupdate: bool,
    pub forget_project_path: bool,
    pub build_target: Option<&'a str>,
    pub extra_unity_args: Vec<&'a str>,
}

pub fn build_unity_compile_check_args(
    project: &Path,
    log_file: &Path,
    options: &UnityCompileCheckOptions<'_>,
) -> Vec<OsString> {
    let mut args = Vec::new();

    if options.accept_apiupdate {
        args.push(OsString::from("-accept-apiupdate"));
    }
    if options.no_graphics {
        args.push(OsString::from("-nographics"));
    }
    if options.forget_project_path {
        args.push(OsString::from("-forgetProjectPath"));
    }
    if let Some(build_target) = options.build_target.filter(|s| !s.trim().is_empty()) {
        args.push(OsString::from("-buildTarget"));
        args.push(OsString::from(build_target));
    }

    args.extend([
        OsString::from("-batchmode"),
        OsString::from("-quit"),
        OsString::from("-projectPath"),
        normalize_for_unity(project).as_os_str().to_os_string(),
        OsString::from("-logFile"),
        normalize_for_unity(log_file).as_os_str().to_os_string(),
    ]);

    for arg in &options.extra_unity_args {
        if !arg.trim().is_empty() {
            args.push(OsString::from(arg));
        }
    }

    args
}

pub fn command_as_strings(editor: &Path, args: &[OsString]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len() + 1);
    out.push(editor.to_string_lossy().to_string());
    out.extend(args.iter().map(|a| a.to_string_lossy().to_string()));
    out
}

fn infer_version_from_editor_path(path: &Path) -> Option<String> {
    let parts: Vec<_> = path.iter().filter_map(|p| p.to_str()).collect();
    for part in parts.iter().rev() {
        if UnityVersion::parse(part).is_some() {
            return Some((*part).to_string());
        }
    }
    None
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct UnityVersion {
    parts: [u32; 4],
    suffix_rank: u8,
    suffix_number: u32,
}

impl UnityVersion {
    pub fn parse(input: &str) -> Option<Self> {
        let re = Regex::new(r"^(\d{4})\.(\d+)\.(\d+)([abfp])?(\d+)?").expect("valid regex");
        let caps = re.captures(input)?;
        let suffix_rank = match caps.get(4).map(|m| m.as_str()) {
            Some("a") => 0,
            Some("b") => 1,
            Some("f") => 2,
            Some("p") => 3,
            _ => 2,
        };
        Some(Self {
            parts: [
                caps[1].parse().ok()?,
                caps[2].parse().ok()?,
                caps[3].parse().ok()?,
                caps.get(5)
                    .and_then(|m| m.as_str().parse().ok())
                    .unwrap_or(0),
            ],
            suffix_rank,
            suffix_number: caps
                .get(5)
                .and_then(|m| m.as_str().parse().ok())
                .unwrap_or(0),
        })
    }
}

impl Ord for UnityVersion {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts
            .cmp(&other.parts)
            .then(self.suffix_rank.cmp(&other.suffix_rank))
            .then(self.suffix_number.cmp(&other.suffix_number))
    }
}

impl PartialOrd for UnityVersion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use tempfile::tempdir;

    #[test]
    fn parses_project_version() {
        assert_eq!(
            parse_project_version("m_EditorVersion: 2022.3.40f1\n"),
            Some("2022.3.40f1".to_string())
        );
    }

    #[test]
    fn sorts_unity_versions() {
        let mut versions = vec![
            UnityVersion::parse("2022.3.8f1").unwrap(),
            UnityVersion::parse("2022.3.40f1").unwrap(),
            UnityVersion::parse("2021.3.30f1").unwrap(),
        ];
        versions.sort();
        assert_eq!(
            versions.last().unwrap(),
            &UnityVersion::parse("2022.3.40f1").unwrap()
        );
    }

    #[test]
    fn resolves_exact_editor_from_fake_hub() {
        let dir = tempdir().unwrap();
        let hub = dir.path().join("Hub").join("Editor");

        let mut cfg = Config::default();
        cfg.unity.search_roots = vec![hub.to_string_lossy().to_string()];

        let exe = join_relative_path(
            &hub.join("2022.3.40f1"),
            cfg.unity.executable_relative_path(),
        );
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"").unwrap();

        let project = ProjectInfo {
            root: dir.path().join("Project"),
            editor_version: "2022.3.40f1".to_string(),
        };
        let resolved = resolve_editor(&cfg, &project).unwrap();
        assert_eq!(resolved.editor_path, exe);
    }

    #[test]
    fn builds_compile_check_args() {
        let options = UnityCompileCheckOptions {
            no_graphics: true,
            accept_apiupdate: true,
            forget_project_path: true,
            build_target: Some("win64"),
            extra_unity_args: vec!["-stackTraceLogType", "Full"],
        };
        let args = command_as_strings(
            Path::new("Unity.exe"),
            &build_unity_compile_check_args(
                Path::new("Project"),
                Path::new("compile.log"),
                &options,
            ),
        );
        assert!(args.contains(&"-batchmode".to_string()));
        assert!(args.contains(&"-quit".to_string()));
        assert!(!args.contains(&"-runTests".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-logFile" && w[1] == "compile.log"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-buildTarget" && w[1] == "win64"));
    }

    #[test]
    fn builds_extended_test_runner_args() {
        let options = UnityCommandOptions {
            test_filter: Some("Foo"),
            category: Some("Fast;!Slow"),
            test_names: Some("A.B;C.D"),
            assembly_names: vec!["Game.Tests", "Game.Editor.Tests"],
            assembly_type: Some(AssemblyType::EditorOnly),
            requires_play_mode: Some(false),
            run_synchronously: true,
            ordered_test_list: Some(Path::new("ordered.txt")),
            test_settings: Some(Path::new("TestSettings.json")),
            player_heartbeat_timeout: Some(42),
            build_player_path: Some(Path::new("Builds/TestPlayer")),
            build_target: Some("win64"),
            no_graphics: true,
            accept_apiupdate: true,
            forget_project_path: true,
            extra_unity_args: vec!["-stackTraceLogType", "Full"],
        };
        let args = command_as_strings(
            Path::new("Unity.exe"),
            &build_unity_command_args(
                Path::new("Project"),
                TestPlatform::EditMode,
                Path::new("results.xml"),
                Path::new("unity.log"),
                &options,
            ),
        );
        assert!(args.contains(&"-accept-apiupdate".to_string()));
        assert!(args.contains(&"-nographics".to_string()));
        assert!(args.contains(&"-forgetProjectPath".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-buildTarget" && w[1] == "win64"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-testNames" && w[1] == "A.B;C.D"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-assemblyNames" && w[1] == "Game.Tests;Game.Editor.Tests"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-assemblyType" && w[1] == "EditorOnly"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-requiresPlayMode" && w[1] == "false"));
        assert!(args.contains(&"-runSynchronously".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-playerHeartbeatTimeout" && w[1] == "42"));
        assert!(args
            .windows(2)
            .any(|w| w[0] == "-stackTraceLogType" && w[1] == "Full"));
    }
}
