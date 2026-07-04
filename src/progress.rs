use crate::cli::CommonArgs;
use crate::error::RunnerError;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

/// Optional progress logger.
///
/// Important: stdout is intentionally never used here, because runner stdout is
/// reserved for the final machine-readable JSON object. Human progress goes to
/// stderr and/or to a file.
pub struct ProgressLogger {
    started: Instant,
    to_stderr: bool,
    file: Option<Mutex<File>>,
}

impl ProgressLogger {
    pub fn from_common_args(args: &CommonArgs) -> Result<Self, RunnerError> {
        let file = match args.progress_file.as_ref() {
            Some(path) => Some(Mutex::new(open_progress_file(path)?)),
            None => None,
        };
        Ok(Self {
            started: Instant::now(),
            to_stderr: args.verbose,
            file,
        })
    }

    pub fn enabled(&self) -> bool {
        self.to_stderr || self.file.is_some()
    }

    pub fn step(&self, message: impl AsRef<str>) {
        if !self.enabled() {
            return;
        }
        let line = format!(
            "[{:>8.2}s] {}",
            self.started.elapsed().as_secs_f64(),
            message.as_ref()
        );
        if self.to_stderr {
            eprintln!("{line}");
        }
        if let Some(file) = &self.file {
            if let Ok(mut file) = file.lock() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
    }
}

fn open_progress_file(path: &Path) -> Result<File, RunnerError> {
    let path = resolve_progress_path(path);
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

fn resolve_progress_path(path: &Path) -> PathBuf {
    let temp_dir = crate::path_util::path_to_json_string(&std::env::temp_dir());
    let expanded = crate::config::expand_env_vars(&path.to_string_lossy())
        .replace("{temp}", &temp_dir)
        .replace("{system_temp}", &temp_dir)
        .replace("{system-temp}", &temp_dir);
    PathBuf::from(expanded)
}
