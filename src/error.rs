use std::fmt;
use std::path::{Path, PathBuf};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    Io { doing: String, source: std::io::Error },
    Parse { path: PathBuf, message: String },
    /// The export is there but has nothing to draw.
    Empty(String),
}

impl Error {
    pub fn io(doing: impl Into<String>, source: std::io::Error) -> Self {
        Self::Io { doing: doing.into(), source }
    }

    pub fn parse(path: &Path, message: impl Into<String>) -> Self {
        Self::Parse { path: path.to_owned(), message: message.into() }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { doing, source } => write!(f, "{doing}: {source}"),
            Self::Parse { path, message } => write!(f, "{}: {message}", path.display()),
            Self::Empty(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Parse { .. } | Self::Empty(_) => None,
        }
    }
}
