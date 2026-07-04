use crate::output::Status;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("runner config error: {0}")]
    Config(String),
    #[error("Unity startup error: {0}")]
    UnityStartup(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML error in {path}: {message}")]
    Toml { path: String, message: String },
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl RunnerError {
    pub fn status(&self) -> Status {
        match self {
            RunnerError::Config(_) | RunnerError::Toml { .. } => Status::RunnerConfigError,
            RunnerError::UnityStartup(_) => Status::UnityStartupError,
            RunnerError::Io(_) | RunnerError::Serialization(_) => Status::UnknownError,
        }
    }

    pub fn exit_code(&self) -> i32 {
        match self.status() {
            Status::RunnerConfigError => 3,
            Status::TestsFailed => 1,
            Status::Passed => 0,
            _ => 2,
        }
    }
}
