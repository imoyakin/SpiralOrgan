use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CoreError {
    InvalidConfig(String),
    NotFound(String),
    PermissionDenied(String),
    Unsupported(String),
    Internal(String),
}

impl Display for CoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(msg) => write!(f, "invalid config: {msg}"),
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::PermissionDenied(msg) => write!(f, "permission denied: {msg}"),
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl Error for CoreError {}
