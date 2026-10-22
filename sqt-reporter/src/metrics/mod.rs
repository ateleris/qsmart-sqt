// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The target descriptor of a qualification specification (SRS-FUNC-003).
//!
//! Measurement happens on the Runner; the Orchestrator reads measured values
//! from the `JobMetrics` of the job-status response (SRS-FUNC-005).

use std::{path::PathBuf, time::Duration};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Local,
    Remote(String),
}

#[derive(Debug, Clone)]
pub struct Target {
    pub binary: PathBuf,
    pub platform: Platform,
    pub input: Option<PathBuf>,
    pub working_dir: PathBuf,
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
