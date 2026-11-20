// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The target descriptor of a qualification specification (SRS-FUNC-003).
//!
//! Measurement happens on the Runner; the Orchestrator reads measured values
//! from the `JobMetrics` of the job-status response (SRS-FUNC-005).

use std::{path::PathBuf, time::Duration};

/// Where a target is executed. Only `Local` is used; execution happens on
/// the Runner either way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    /// The default.
    Local,
    /// A named remote platform. Not used by the Orchestrator.
    Remote(String),
}

/// What a specification runs and with which limits.
#[derive(Debug, Clone)]
pub struct Target {
    /// Path of the binary under test. The Runner configures the binary, so the
    /// Orchestrator leaves this empty; the report shows it as given.
    pub binary: PathBuf,
    /// Execution platform; always [`Platform::Local`].
    pub platform: Platform,
    /// Input file of the specification, sent with the job.
    pub input: Option<PathBuf>,
    /// Working directory; not used by the Orchestrator.
    pub working_dir: PathBuf,
    /// Upper bound for the job, from the specification or the configured
    /// default.
    pub timeout: Duration,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            binary: PathBuf::new(),
            platform: Platform::Local,
            input: None,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies: SRS-PERF-004
    #[test]
    fn target_default_has_30s_timeout() {
        let target = Target::default();
        assert_eq!(target.timeout, Duration::from_secs(30));
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn target_default_uses_local_platform() {
        let target = Target::default();
        assert_eq!(target.platform, Platform::Local);
    }
}
