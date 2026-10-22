// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Traceability matrix builder (T-053).
//!
//! Builds a traceability matrix from the baseline `RequirementSet` and `RunResult`,
//! then writes it as both `traceability.json` and `traceability.txt`.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatus {
    /// Covered by spec + job — outcome recorded.
    Tested,
    /// Non-Test verification method; no job needed.
    CompliantByDeclaration,
    /// Testable but no spec matched.
    Uncovered,
}

#[derive(Debug, Clone, Serialize)]
pub struct TraceEntry {
    pub uid: String,
    pub title: String,
    pub status: TraceStatus,
    pub verification_methods: Vec<String>,
    /// `"PASS"`, `"FAIL"`, `"INFRA_ERROR"`, or `None` when no result exists.
    pub outcome: Option<String>,
    /// Evidence ids of the matching result: paths relative to the run
    /// directory, listed in `evidence/manifest.json`.
    pub evidence_paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct TraceabilityMatrix {
    pub schema_version: &'static str,
    pub generated_at: String,
    pub summary: TraceSummary,
    pub entries: Vec<TraceEntry>,
}

#[derive(Debug, Serialize)]
pub struct TraceSummary {
    pub total: usize,
    pub tested: usize,
    pub compliant_by_declaration: usize,
    pub uncovered: usize,
    pub pass: usize,
    pub fail: usize,
    pub infra_error: usize,
}

/// Build a traceability matrix from baseline requirements and run results.
///
/// - Testable requirements with a matching `SpecResult` → `Tested`
/// - Compliant-by-declaration requirements → `CompliantByDeclaration`
/// - Testable requirements with no matching spec → `Uncovered`
pub fn build(
    req_set: &crate::baseline::RequirementSet,
    run_result: &crate::specs::RunResult,
    generated_at: &str,
) -> TraceabilityMatrix {
    let mut entries = Vec::new();

    // Testable requirements — either Tested or Uncovered.
    for req in &req_set.testable {
        let spec_result = run_result.results.iter().find(|r| r.spec.requirement_uid == req.uid);
        let (outcome, evidence) = match spec_result {
            Some(sr) => {
                let outcome_str = match sr.outcome {
                    crate::specs::OutcomeKind::Pass => "PASS",
                    crate::specs::OutcomeKind::Fail => "FAIL",
                    crate::specs::OutcomeKind::InfraError => "INFRA_ERROR",
                }
                .to_string();
                (Some(outcome_str), sr.evidence.clone())
            }
            None => (None, Vec::new()),
        };
        let status =
            if spec_result.is_some() { TraceStatus::Tested } else { TraceStatus::Uncovered };
        entries.push(TraceEntry {
            uid: req.uid.clone(),
            title: req.title.clone(),
            status,
            verification_methods: req.verification_methods.clone(),
            outcome,
            evidence_paths: evidence,
        });
    }

    // Compliant-by-declaration requirements.
    for req in &req_set.compliant_by_declaration {
        entries.push(TraceEntry {
            uid: req.uid.clone(),
            title: req.title.clone(),
            status: TraceStatus::CompliantByDeclaration,
            verification_methods: req.verification_methods.clone(),
            outcome: None,
            evidence_paths: Vec::new(),
        });
    }

    let total = entries.len();
    let tested = entries.iter().filter(|e| e.status == TraceStatus::Tested).count();
    let cbd = entries.iter().filter(|e| e.status == TraceStatus::CompliantByDeclaration).count();
    let uncovered = entries.iter().filter(|e| e.status == TraceStatus::Uncovered).count();
    let pass = entries.iter().filter(|e| e.outcome.as_deref() == Some("PASS")).count();
    let fail = entries.iter().filter(|e| e.outcome.as_deref() == Some("FAIL")).count();
    let infra = entries.iter().filter(|e| e.outcome.as_deref() == Some("INFRA_ERROR")).count();

    TraceabilityMatrix {
        schema_version: "1.0",
        generated_at: generated_at.to_string(),
        summary: TraceSummary {
            total,
            tested,
            compliant_by_declaration: cbd,
            uncovered,
            pass,
            fail,
            infra_error: infra,
        },
        entries,
    }
}

/// Write machine-readable traceability matrix as JSON.
///
/// # Errors
///
/// Returns an error if the parent directory cannot be created or the file
/// cannot be written.
pub fn write_json(matrix: &TraceabilityMatrix, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(matrix)?;
    std::fs::write(path, json)?;
    Ok(())
}

/// Write human-readable traceability matrix as plain text.
///
/// # Errors
///
/// Returns an error if the parent directory cannot be created or the file
/// cannot be written.
pub fn write_text(matrix: &TraceabilityMatrix, path: &Path) -> Result<()> {
    use std::fmt::Write as _;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    out.push_str("TRACEABILITY MATRIX\n");
    writeln!(out, "Generated: {}", matrix.generated_at)?;
    writeln!(
        out,
        "Total: {} | Tested: {} | Compliant-by-declaration: {} | Uncovered: {}",
        matrix.summary.total,
        matrix.summary.tested,
        matrix.summary.compliant_by_declaration,
        matrix.summary.uncovered
    )?;
    writeln!(
        out,
        "Results: PASS={} FAIL={} INFRA_ERROR={}\n",
        matrix.summary.pass, matrix.summary.fail, matrix.summary.infra_error
    )?;
    for entry in &matrix.entries {
        let status = match entry.status {
            TraceStatus::Tested => entry.outcome.as_deref().unwrap_or("UNTESTED"),
            TraceStatus::CompliantByDeclaration => "COMPLIANT",
            TraceStatus::Uncovered => "UNCOVERED",
        };
        writeln!(out, "[{status:12}] {} \u{2014} {}", entry.uid, entry.title)?;
    }
    std::fs::write(path, out)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use crate::baseline::{Requirement, RequirementSet};
    use crate::constraint::{Constraint, ConstraintOutcome, Op};
    use crate::metrics::{Platform, Target};
    use crate::specs::{RunResult, SpecResult, TestSpec};

    use super::*;

    fn empty_req_set() -> RequirementSet {
        RequirementSet { testable: vec![], compliant_by_declaration: vec![], unclassified: vec![] }
    }

    fn empty_run_result() -> RunResult {
        RunResult {
            results: vec![],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            config_version: "1.0.0".to_string(),
            interrupted: false,
            requirement_set: None,
            discovery: None,
        }
    }

    fn make_req(uid: &str, verification: &[&str]) -> Requirement {
        Requirement {
            uid: uid.to_string(),
            title: format!("Title for {uid}"),
            status: "Active".to_string(),
            verification_methods: verification
                .iter()
                .map(std::string::ToString::to_string)
                .collect(),
        }
    }

    fn make_target() -> Target {
        Target {
            binary: PathBuf::from("/bin/app"),
            input: None,
            platform: Platform::Local,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }

    fn make_spec(uid: &str) -> TestSpec {
        TestSpec {
            requirement_uid: uid.to_string(),
            target: make_target(),
            constraints: vec![Constraint::return_code(Op::Eq, 0)],
            ..Default::default()
        }
    }

    fn passing_outcome() -> ConstraintOutcome {
        ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence: HashMap::new(),
        }
    }

    // -------------------------------------------------------------------------

    /// Verifies: SRS-FUNC-010
    #[test]
    fn build_empty_gives_empty_entries() {
        let req_set = empty_req_set();
        let run_result = empty_run_result();
        let matrix = build(&req_set, &run_result, "2026-01-01T00:00:00Z");
        assert!(matrix.entries.is_empty());
        assert_eq!(matrix.summary.total, 0);
        assert_eq!(matrix.summary.tested, 0);
        assert_eq!(matrix.summary.compliant_by_declaration, 0);
        assert_eq!(matrix.summary.uncovered, 0);
    }

    /// Verifies: SRS-FUNC-010
    #[test]
    fn build_matched_spec_maps_to_tested_with_outcome() {
        let req_set = RequirementSet {
            testable: vec![make_req("REQ-001", &["Test"])],
            compliant_by_declaration: vec![],
            unclassified: vec![],
        };
        let spec_result = SpecResult::new(make_spec("REQ-001"), vec![passing_outcome()]);
        let run_result = RunResult {
            results: vec![spec_result],
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            config_version: "1.0.0".to_string(),
            interrupted: false,
            requirement_set: None,
            discovery: None,
        };
        let matrix = build(&req_set, &run_result, "2026-01-01T00:00:00Z");
        assert_eq!(matrix.entries.len(), 1);
        let entry = &matrix.entries[0];
        assert_eq!(entry.uid, "REQ-001");
        assert_eq!(entry.status, TraceStatus::Tested);
        assert_eq!(entry.outcome.as_deref(), Some("PASS"));
        assert_eq!(matrix.summary.tested, 1);
        assert_eq!(matrix.summary.pass, 1);
    }

    /// Verifies: SRS-FUNC-010
    #[test]
    fn build_unmatched_testable_maps_to_uncovered() {
        let req_set = RequirementSet {
            testable: vec![make_req("REQ-002", &["Test"])],
            compliant_by_declaration: vec![],
            unclassified: vec![],
        };
        let run_result = empty_run_result();
        let matrix = build(&req_set, &run_result, "2026-01-01T00:00:00Z");
        assert_eq!(matrix.entries.len(), 1);
        let entry = &matrix.entries[0];
        assert_eq!(entry.uid, "REQ-002");
        assert_eq!(entry.status, TraceStatus::Uncovered);
        assert!(entry.outcome.is_none());
        assert_eq!(matrix.summary.uncovered, 1);
        assert_eq!(matrix.summary.tested, 0);
    }

    /// Verifies: SRS-FUNC-010
    #[test]
    fn build_compliant_by_declaration_entries() {
        let req_set = RequirementSet {
            testable: vec![],
            compliant_by_declaration: vec![make_req("REQ-003", &["Analysis"])],
            unclassified: vec![],
        };
        let run_result = empty_run_result();
        let matrix = build(&req_set, &run_result, "2026-01-01T00:00:00Z");
        assert_eq!(matrix.entries.len(), 1);
        let entry = &matrix.entries[0];
        assert_eq!(entry.uid, "REQ-003");
        assert_eq!(entry.status, TraceStatus::CompliantByDeclaration);
        assert!(entry.outcome.is_none());
        assert_eq!(matrix.summary.compliant_by_declaration, 1);
    }

    /// Verifies: SRS-FUNC-010, SRS-DATA-002
    #[test]
    fn write_json_creates_valid_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traceability.json");
        let matrix = build(&empty_req_set(), &empty_run_result(), "2026-01-01T00:00:00Z");
        write_json(&matrix, &path).expect("write_json must succeed");
        assert!(path.exists(), "traceability.json must be created");
        let content = std::fs::read_to_string(&path).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&content).expect("must be valid JSON");
        assert_eq!(parsed["schema_version"], "1.0");
        assert!(parsed["entries"].is_array());
    }

    /// Verifies: SRS-FUNC-010, SRS-DATA-002
    #[test]
    fn write_text_creates_file_with_expected_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traceability.txt");
        let matrix = build(&empty_req_set(), &empty_run_result(), "2026-06-05T12:00:00Z");
        write_text(&matrix, &path).expect("write_text must succeed");
        assert!(path.exists(), "traceability.txt must be created");
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.starts_with("TRACEABILITY MATRIX\n"), "must start with header");
        assert!(content.contains("Generated: 2026-06-05T12:00:00Z"), "must contain timestamp");
        assert!(content.contains("Total:"), "must contain summary line");
    }
}
