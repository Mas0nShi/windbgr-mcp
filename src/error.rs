use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid configuration: {0}")]
    Config(String),

    #[error("cdb executable not found: {0}")]
    CdbNotFound(String),

    #[error("cdb session not found: {0}")]
    SessionNotFound(String),

    #[error("cdb session limit reached (max = {0})")]
    SessionLimit(usize),

    #[error("invalid session state: {current} — {action} not allowed")]
    InvalidState { current: String, action: String },

    #[error("cdb session exited unexpectedly: {0}")]
    CdbExited(String),

    #[error("operation timed out after {0} ms")]
    Timeout(u64),

    #[error("windows api error: {0}")]
    WindowsApi(String),

    #[error("{0}")]
    Other(String),
}

impl Error {
    pub fn other(msg: impl Into<String>) -> Self {
        Error::Other(msg.into())
    }
}

impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Other(value.to_string())
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Error::Config(value.to_string())
    }
}
