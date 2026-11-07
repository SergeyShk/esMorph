use std::fmt;

/// Error of reading or building a dictionary
#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    /// Input or output error
    Io(std::io::Error),
    /// The data is no dictionary of esMorph or is damaged
    Format(String),
    /// Wrong data for building a dictionary
    Build(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(err) => write!(f, "{err}"),
            Error::Format(msg) => write!(f, "wrong dictionary format: {msg}"),
            Error::Build(msg) => write!(f, "dictionary build error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}
