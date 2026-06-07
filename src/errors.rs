use std::{error::Error, fmt::Display, path::PathBuf};

#[derive(Debug)]
pub enum NGitError {
    IO(std::io::Error),
    Uninitialized(PathBuf),
    MissingArgument(String, String),
    UnexpectedDataType(String, String),
    MissingDataType,
    InvalidDataType(String),
    InvalidObject(String),
    InvalidOid(String),
    InvalidRefName(String),
    InvalidRepoPath(PathBuf),
    // InvalidFileRead(String)
    NoArgumentExpected,
    InvalidCommit(String),
    // MissingHead,
    Unresolvable(String),
    OperationFailed(String),
    SystemError(String, String),
    RemoteUninitialized(String),
    NoForcePush(String),
}

impl Display for NGitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NGitError::IO(err) => writeln!(f, "{}", err),
            NGitError::Uninitialized(p) => writeln!(f, "no repository initialized at {:?}", p),
            NGitError::MissingArgument(cmd, ty) => {
                write!(f, "missing '{}' argument for '{}' command", ty, cmd)
            }
            NGitError::MissingDataType => write!(f, "a datatype is required, can't be _"),
            NGitError::InvalidDataType(s) => write!(f, "{} is not a valid datatype", s),
            NGitError::UnexpectedDataType(expected, received) => {
                write!(f, "expected {} data type, received {}", expected, received)
            }
            NGitError::InvalidObject(h) => write!(f, "corrupted objected at {}", h),
            NGitError::InvalidOid(oid) => write!(f, "{} is not a valid object id", oid),
            NGitError::InvalidRefName(name) => write!(f, "{} is not a valid ref name", name),
            NGitError::InvalidRepoPath(path) => {
                write!(f, "{:?} is not a valid repo-relative path", path)
            }
            // NGitError::InvalidFileRead(s) => write!(f, "unable to read file at {}", s)
            NGitError::NoArgumentExpected => {
                write!(f, "no argument expected by the specified command")
            }
            NGitError::InvalidCommit(oid) => write!(f, "commit {} is invalid", oid),
            // NGitError::MissingHead => write!(f, "missing HEAD"),
            NGitError::Unresolvable(s) => {
                write!(f, "expected a SHA1 digest or a reference; received {}", s)
            }
            NGitError::OperationFailed(s) => write!(f, "'{}' operation failed", s),
            NGitError::SystemError(s, msg) => write!(f, "'{}' failed \n\n {}", s, msg),
            NGitError::RemoteUninitialized(s) => write!(f, "no ngit repo found at {}", s),
            NGitError::NoForcePush(s) => write!(f, "no force push allowed. {s}"),
        }
    }
}

impl Error for NGitError {}

impl From<std::io::Error> for NGitError {
    fn from(value: std::io::Error) -> Self {
        NGitError::IO(value)
    }
}

impl From<std::time::SystemTimeError> for NGitError {
    fn from(value: std::time::SystemTimeError) -> Self {
        NGitError::SystemError("systemtime".into(), value.to_string())
    }
}
