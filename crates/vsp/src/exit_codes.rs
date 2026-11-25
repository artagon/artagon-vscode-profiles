//! Process exit codes and structured error types for `vsp`.
//!
//! Exit code semantics are locked by ADR 0001:
//! - `0`: Ok / clean check / ordinary preview / `--help` / `--version`
//! - `1`: Operational, audit, or validation failure
//! - `2`: Target missing (path, profile, preset, pack, or bundle)
//! - `3`: Complete negative check (`detect` no match, or `diff --check` verified drift)
//! - `4`: Usage, config, selector, or module composition/conflict error
//! - `5`: Safety guard refusal before write
//! - `6`: Retryable lock, preimage, or partial apply error
//! - `7`: Incomplete observation or verification unavailable

use std::fmt;
use thiserror::Error;

/// Canonical process exit codes returned by the `vsp` CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExitCode {
    /// Success, clean check, ordinary preview, `--help`, or `--version`.
    Ok = 0,
    /// Operational, audit, or validation failure.
    Operational = 1,
    /// Requested target is missing (path, profile, preset, pack, or bundle).
    TargetMissing = 2,
    /// Complete negative check (`detect` found no match, or `diff --check` found drift).
    NegativeCheck = 3,
    /// Usage error, invalid configuration, missing selector, or module composition conflict.
    UsageOrConfig = 4,
    /// Safety guard refusal before mutating target state.
    SafetyRefusal = 5,
    /// Retryable lock contention, preimage mismatch, or partial apply failure.
    RetryableLock = 6,
    /// Incomplete observation or verification unavailable (e.g., failed `code --list-extensions`).
    IncompleteObservation = 7,
}

impl ExitCode {
    /// Returns the raw POSIX exit status code as `i32`.
    #[must_use]
    pub const fn as_i32(self) -> i32 {
        self as i32
    }
}

impl fmt::Display for ExitCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_i32())
    }
}

/// Unified error type carrying a deterministic [`ExitCode`].
#[derive(Debug, Error)]
pub enum VspError {
    /// Operational, IO, parse, or validation failure (exit `1`).
    #[error("{0}")]
    Operational(String),

    /// Missing target path, profile, preset, pack, or bundle (exit `2`).
    #[error("{0}")]
    TargetMissing(String),

    /// Complete negative check result (exit `3`).
    #[error("{0}")]
    NegativeCheck(String),

    /// Invalid CLI usage, configuration, selector, or DAG composition error (exit `4`).
    #[error("{0}")]
    UsageOrConfig(String),

    /// Safety guard refusal before mutation (exit `5`).
    #[error("{0}")]
    SafetyRefusal(String),

    /// Retryable lock or preimage mismatch (exit `6`).
    #[error("{0}")]
    RetryableLock(String),

    /// Incomplete observation or unavailable external verification (exit `7`).
    #[error("{0}")]
    IncompleteObservation(String),
}

impl VspError {
    /// Maps this error variant to its authoritative [`ExitCode`].
    #[must_use]
    pub const fn exit_code(&self) -> ExitCode {
        match self {
            Self::Operational(_) => ExitCode::Operational,
            Self::TargetMissing(_) => ExitCode::TargetMissing,
            Self::NegativeCheck(_) => ExitCode::NegativeCheck,
            Self::UsageOrConfig(_) => ExitCode::UsageOrConfig,
            Self::SafetyRefusal(_) => ExitCode::SafetyRefusal,
            Self::RetryableLock(_) => ExitCode::RetryableLock,
            Self::IncompleteObservation(_) => ExitCode::IncompleteObservation,
        }
    }
}

/// Convenience result alias used throughout `vsp`.
pub type VspResult<T> = Result<T, VspError>;
