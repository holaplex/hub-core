use std::error::Error;

/// Information about the severity of an error
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The error arises from user input, the current operation should not be
    /// retried as the request is invalid
    User,
    /// The error is recoverable and a retry should be attempted
    Transient,
    /// The error is unrecoverable, the current operation should not be retried
    Permanent,
    /// The error is unrecoverable and the service should terminate immediately
    Fatal,
}

/// An error containing information about its severity
pub trait Triage: Error {
    /// Get the severity level of this error
    fn severity(&self) -> Severity;
}
