//! Error types for AiDb storage engine.

use std::fmt;
use std::io;

/// The result type used throughout AiDb.
pub type Result<T> = std::result::Result<T, Error>;

/// The error type for AiDb operations.
#[derive(Debug)]
pub enum Error {
    /// An I/O error occurred.
    Io(io::Error),

    /// Data corruption was detected.
    Corruption(String),

    /// The requested key was not found.
    NotFound(String),

    /// An invalid argument was provided.
    InvalidArgument(String),

    /// A feature or function is not yet implemented.
    NotImplemented(String),

    /// The database is in an invalid state.
    InvalidState(String),

    /// A serialization or deserialization error occurred.
    Serialization(String),

    /// A checksum mismatch was detected.
    ChecksumMismatch {
        /// The expected checksum value.
        expected: u32,
        /// The actual checksum value.
        actual: u32,
    },

    /// The database or file is already in use.
    AlreadyExists(String),

    /// An internal error occurred.
    Internal(String),
}

impl Error {
    /// Creates a new corruption error.
    pub fn corruption(msg: impl Into<String>) -> Self {
        Error::Corruption(msg.into())
    }

    /// Creates a new not found error.
    pub fn not_found(msg: impl Into<String>) -> Self {
        Error::NotFound(msg.into())
    }

    /// Creates a new invalid argument error.
    pub fn invalid_argument(msg: impl Into<String>) -> Self {
        Error::InvalidArgument(msg.into())
    }

    /// Creates a new internal error.
    pub fn internal(msg: impl Into<String>) -> Self {
        Error::Internal(msg.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "IO error: {}", e),
            Error::Corruption(msg) => write!(f, "Data corruption: {}", msg),
            Error::NotFound(msg) => write!(f, "Not found: {}", msg),
            Error::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            Error::NotImplemented(msg) => write!(f, "Not implemented: {}", msg),
            Error::InvalidState(msg) => write!(f, "Invalid state: {}", msg),
            Error::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            Error::ChecksumMismatch { expected, actual } => {
                write!(f, "Checksum mismatch: expected {:#x}, got {:#x}", expected, actual)
            }
            Error::AlreadyExists(msg) => write!(f, "Already exists: {}", msg),
            Error::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<bincode::Error> for Error {
    fn from(err: bincode::Error) -> Self {
        Error::Serialization(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::corruption("test corruption");
        assert_eq!(err.to_string(), "Data corruption: test corruption");

        let err = Error::ChecksumMismatch { expected: 0x12345678, actual: 0x87654321 };
        assert!(err.to_string().contains("0x12345678"));
        assert!(err.to_string().contains("0x87654321"));
    }

    #[test]
    fn test_error_from_io() {
        let io_err = io::Error::new(io::ErrorKind::NotFound, "file not found");
        let err: Error = io_err.into();
        assert!(matches!(err, Error::Io(_)));
    }
}
