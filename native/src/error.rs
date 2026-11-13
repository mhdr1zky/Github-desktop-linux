#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Git: {0}")]
    Git(#[from] git2::Error),
    #[error("Filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("Preferences: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Unsupported(String),
    #[error("{0}")]
    Invalid(String),
    #[error("Operation cancelled")]
    Cancelled,
}

pub type Result<T> = std::result::Result<T, Error>;
