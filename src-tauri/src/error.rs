use serde::Serialize;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpiceError {
    #[error("{0}")]
    User(String),
    #[error("The configured path does not exist: {0}")]
    MissingPath(PathBuf),
    #[error(
        "Codex is still running. Save your work and fully quit every Codex window and CLI session."
    )]
    CodexRunning,
    #[error("Codex reopened during this handoff. Close Codex before trying again.")]
    CodexReopened,
    #[error("This Codex data format is not supported for restore: {0}")]
    UnsupportedCodex(String),
    #[error("The cloud snapshot is incomplete or corrupt: {0}")]
    CorruptSnapshot(String),
    #[error("This computer has not pulled snapshot {0}. Pull before pushing to avoid divergent histories.")]
    PullRequired(String),
    #[error("The operation was cancelled safely.")]
    Cancelled,
    #[error("A recovery operation is pending. Resolve it from Recovery before starting another handoff.")]
    PendingRecovery,
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Codex database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("Invalid stored data: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Could not match an exclusion pattern: {0}")]
    Pattern(#[from] globset::Error),
    #[error("{0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl Serialize for SpiceError {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type Result<T> = std::result::Result<T, SpiceError>;

impl From<&str> for SpiceError {
    fn from(value: &str) -> Self {
        Self::User(value.to_string())
    }
}

impl From<String> for SpiceError {
    fn from(value: String) -> Self {
        Self::User(value)
    }
}
