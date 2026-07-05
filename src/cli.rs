use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "unity-test-runner")]
#[command(about = "Run Unity Test Framework and emit compact JSON")]
#[command(version)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    /// Run Unity tests and return JSON.
    Run(RunArgs),
    /// Open Unity in batchmode and verify that project scripts compile.
    CompileCheck(CompileCheckArgs),
    /// Validate config, Unity project and Unity editor path.
    Doctor(CommonArgs),
    /// Print the resolved config after merging defaults/project/CLI.
    PrintConfig(CommonArgs),
    /// Run a local MCP server over stdio exposing Unity runner tools.
    Mcp,
    /// Print runner version and JSON schema version.
    Version,
}

#[derive(Debug, Args, Clone)]
pub struct CommonArgs {
    /// Unity project root. Defaults to current directory.
    #[arg(long, value_name = "path", default_value = ".")]
    pub project: PathBuf,

    /// Explicit Unity.exe override.
    #[arg(long, value_name = "path")]
    pub editor: Option<PathBuf>,

    /// Extra Unity Hub Editor search root. Repeatable.
    #[arg(long = "editor-base", value_name = "path")]
    pub editor_base: Vec<PathBuf>,

    /// Extra/explicit TOML config path.
    #[arg(long, value_name = "path")]
    pub config: Option<PathBuf>,

    /// Output format.
    #[arg(long, value_enum)]
    pub format: Option<OutputFormat>,

    /// Keep artifacts regardless of status.
    #[arg(long)]
    pub keep: bool,

    /// Override timeout in seconds.
    #[arg(long, value_name = "seconds")]
    pub timeout: Option<u64>,

    /// Override log tail line count.
    #[arg(long = "log-tail", value_name = "lines")]
    pub log_tail: Option<usize>,

    /// Override artifacts directory. Supports {temp}, {project}, {project_hash}, {platform}.
    #[arg(long = "artifact-dir", value_name = "path")]
    pub artifact_dir: Option<PathBuf>,

    /// Print human-readable progress stages to stderr. Stdout remains final JSON only.
    #[arg(long, short = 'v', alias = "progress")]
    pub verbose: bool,

    /// Append human-readable progress stages to this file. Supports env vars and {temp}.
    #[arg(long = "progress-file", value_name = "path")]
    pub progress_file: Option<PathBuf>,
}

#[derive(Debug, Args, Clone)]
pub struct CompileCheckArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Add Unity -nographics. Useful on headless machines; logs still go to -logFile.
    #[arg(long = "no-graphics")]
    pub no_graphics: bool,

    /// Add Unity -accept-apiupdate so API Updater can run in batchmode.
    #[arg(long = "accept-apiupdate")]
    pub accept_apiupdate: bool,

    /// Add Unity -forgetProjectPath so the project is not saved in Hub history.
    #[arg(long = "forget-project-path")]
    pub forget_project_path: bool,

    /// Unity -buildTarget name, e.g. win64, android, ios, webgl.
    #[arg(long = "build-target", value_name = "name")]
    pub build_target: Option<String>,

    /// Pass an extra raw argument to Unity. Repeat for flag/value pairs.
    #[arg(long = "unity-arg", value_name = "arg")]
    pub extra_unity_args: Vec<String>,

    /// Build and print Unity command without running it.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Args, Clone)]
pub struct RunArgs {
    #[command(flatten)]
    pub common: CommonArgs,

    /// Test platform.
    #[arg(long, value_enum)]
    pub platform: Option<TestPlatform>,

    /// Unity -testFilter.
    #[arg(long = "filter", value_name = "test-filter")]
    pub test_filter: Option<String>,

    /// Unity -testCategory, e.g. Smoke or A;B. Supports negation, e.g. !Slow.
    #[arg(long, value_name = "categories")]
    pub category: Option<String>,

    /// Unity -testNames: semicolon-separated full test names.
    #[arg(long = "test-names", value_name = "names")]
    pub test_names: Option<String>,

    /// Unity -assemblyNames. Repeatable; values are joined with semicolons.
    #[arg(long = "assembly", alias = "assembly-name", value_name = "assembly")]
    pub assembly_names: Vec<String>,

    /// Unity -assemblyType.
    #[arg(long = "assembly-type", value_enum)]
    pub assembly_type: Option<AssemblyType>,

    /// Unity -requiresPlayMode=true|false.
    #[arg(long = "requires-play-mode", value_name = "true|false")]
    pub requires_play_mode: Option<bool>,

    /// Unity -runSynchronously. EditMode only; filters out multi-frame UnityTest cases.
    #[arg(long = "run-synchronously")]
    pub run_synchronously: bool,

    /// Unity -orderedTestListFile path.
    #[arg(long = "ordered-test-list", value_name = "path")]
    pub ordered_test_list: Option<PathBuf>,

    /// Unity -testSettingsFile path.
    #[arg(long = "test-settings", value_name = "path")]
    pub test_settings: Option<PathBuf>,

    /// Unity -playerHeartbeatTimeout seconds for player-based test runs.
    #[arg(long = "player-heartbeat-timeout", value_name = "seconds")]
    pub player_heartbeat_timeout: Option<u64>,

    /// Unity -buildPlayerPath path for player-based test builds.
    #[arg(long = "build-player-path", value_name = "path")]
    pub build_player_path: Option<PathBuf>,

    /// Unity -buildTarget name, e.g. win64, android, ios, webgl.
    #[arg(long = "build-target", value_name = "name")]
    pub build_target: Option<String>,

    /// Add Unity -nographics. Useful on headless machines; logs still go to -logFile.
    #[arg(long = "no-graphics")]
    pub no_graphics: bool,

    /// Add Unity -accept-apiupdate so API Updater can run in batchmode.
    #[arg(long = "accept-apiupdate")]
    pub accept_apiupdate: bool,

    /// Add Unity -forgetProjectPath so the project is not saved in Hub history.
    #[arg(long = "forget-project-path")]
    pub forget_project_path: bool,

    /// Pass an extra raw argument to Unity. Repeat for flag/value pairs.
    #[arg(long = "unity-arg", value_name = "arg")]
    pub extra_unity_args: Vec<String>,

    /// Build and print Unity command without running it.
    #[arg(long)]
    pub dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum TestPlatform {
    #[value(name = "EditMode", alias = "editmode", alias = "edit-mode")]
    EditMode,
    #[value(name = "PlayMode", alias = "playmode", alias = "play-mode")]
    PlayMode,
    /// Runner-level aggregate mode: run EditMode and PlayMode sequentially.
    #[value(name = "All", alias = "all", alias = "both")]
    All,
}

impl Default for TestPlatform {
    fn default() -> Self {
        Self::EditMode
    }
}

impl fmt::Display for TestPlatform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TestPlatform::EditMode => f.write_str("EditMode"),
            TestPlatform::PlayMode => f.write_str("PlayMode"),
            TestPlatform::All => f.write_str("All"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
pub enum AssemblyType {
    #[value(name = "EditorOnly", alias = "editor-only", alias = "editoronly")]
    EditorOnly,
    #[value(name = "EditorAndPlatforms", alias = "editor-and-platforms", alias = "editorandplatforms")]
    EditorAndPlatforms,
}

impl fmt::Display for AssemblyType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AssemblyType::EditorOnly => f.write_str("EditorOnly"),
            AssemblyType::EditorAndPlatforms => f.write_str("EditorAndPlatforms"),
        }
    }
}

impl std::str::FromStr for TestPlatform {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "editmode" | "edit-mode" => Ok(Self::EditMode),
            "playmode" | "play-mode" => Ok(Self::PlayMode),
            "all" | "both" => Ok(Self::All),
            _ => Err(format!("unsupported test platform: {s}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    /// Print only `ok` on success; print compact JSON on failure.
    #[value(name = "minimal")]
    Minimal,
    /// Print `{"ok":true}` on success; print compact JSON on failure.
    #[value(name = "minimal-json")]
    MinimalJson,
    CompactJson,
    PrettyJson,
}

impl Default for OutputFormat {
    fn default() -> Self {
        Self::MinimalJson
    }
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Minimal => f.write_str("minimal"),
            OutputFormat::MinimalJson => f.write_str("minimal-json"),
            OutputFormat::CompactJson => f.write_str("compact-json"),
            OutputFormat::PrettyJson => f.write_str("pretty-json"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "minimal" => Ok(Self::Minimal),
            "minimal-json" => Ok(Self::MinimalJson),
            "compact-json" => Ok(Self::CompactJson),
            "pretty-json" => Ok(Self::PrettyJson),
            _ => Err(format!("unsupported output format: {s}")),
        }
    }
}
