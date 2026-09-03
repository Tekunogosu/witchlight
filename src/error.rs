use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io { doing: String, source: std::io::Error },
    Parse { path: PathBuf, message: String },
    /// The export is there but has nothing to draw.
    Empty(String),
    /// The settings do not say enough to act on, and only a person can settle it.
    Config(String),
    /// The map's own database refused something. What was being done and what
    /// it said, since the underlying error type is the library's own.
    Database { doing: String, message: String },
}

impl Error {
    pub fn io(doing: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io { doing: doing.into(), source }
    }

    pub fn parse(path: &Path, message: impl Into<String>) -> Self {
        Self::Parse { path: path.to_owned(), message: message.into() }
    }

    pub fn config(message: impl Into<String>) -> Self {
        Self::Config(message.into())
    }

    pub fn database(doing: impl Into<String>, error: rusqlite::Error) -> Self {
        Self::Database { doing: doing.into(), message: error.to_string() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { doing, source } => write!(f, "{doing}: {source}"),
            Self::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Empty(message) | Self::Config(message) => f.write_str(message),
            Self::Database { doing, message } => write!(f, "{doing}: {message}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { .. } | Self::Empty(_) | Self::Config(_) | Self::Database { .. } => None,
        }
    }
}
