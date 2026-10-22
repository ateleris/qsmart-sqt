// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! `StrictDoc` requirement baseline checker (T-043).
//!
//! Fetches qualification documents from the Runner, parses ECSS `[REQUIREMENT]`
//! blocks, and matches them against the loaded test specs. Testable requirements
//! without a matching spec are UNCOVERED → non-zero exit before any jobs run.

use anyhow::{Result, bail};
use base64::Engine as _;
use serde::Serialize;
use sqt_types::DocsResponse;

use crate::evidence::{EvidenceMeta, EvidenceStore};

#[derive(Debug, Clone)]
pub struct Requirement {
    pub uid: String,
    pub title: String,
    pub status: String,
    pub verification_methods: Vec<String>,
}

impl Requirement {
    /// True if VERIFICATION contains "Test" (case-insensitive).
    pub fn is_testable(&self) -> bool {
        self.verification_methods.iter().any(|m| m.trim().eq_ignore_ascii_case("test"))
    }
}

#[derive(Debug, Clone)]
pub struct RequirementSet {
    pub testable: Vec<Requirement>,
    pub compliant_by_declaration: Vec<Requirement>,
    pub unclassified: Vec<Requirement>,
}

/// What a run obtained from the Runner's discovery operation (SRS-FUNC-018),
/// as recorded in the report.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveryRecord {
    /// `obtained`, or `empty` when the Runner declared no requirement documents.
    pub status: &'static str,
    /// Evidence id of the raw `GET /api/v1/docs` response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_evidence_id: Option<String>,
    pub documents: Vec<DiscoveredFile>,
    pub attachments: Vec<DiscoveredFile>,
    pub testable: usize,
    pub compliant_by_declaration: usize,
    pub unclassified: usize,
}

/// One document or attachment served by the Runner, preserved verbatim.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredFile {
    pub filename: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,
    /// Evidence id of the preserved file; `None` if it could not be written.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
    /// Set when the file could not be preserved as served, e.g. invalid base64.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Preserve every document and attachment of `docs` verbatim under
/// `evidence/discovery/` (attachments base64-decoded), and summarise the
/// discovery for the report. `response_evidence_id` is the raw response,
/// already persisted with the other Runner responses.
pub fn record_discovery(
    docs: &DocsResponse,
    req_set: &RequirementSet,
    response_evidence_id: Option<String>,
    store: &mut EvidenceStore,
) -> DiscoveryRecord {
    let mut keep = |folder: &str, name: &str, content: &[u8], kind: &'static str| {
        store
            .store(folder, name, content, EvidenceMeta { kind, ..EvidenceMeta::default() })
            .map_err(|e| tracing::warn!("cannot preserve discovered file {name}: {e}"))
            .ok()
    };

    let documents = docs
        .documents
        .iter()
        .map(|doc| DiscoveredFile {
            filename: doc.filename.clone(),
            title: doc.title.clone(),
            content_type: None,
            evidence_id: keep(
                "discovery/documents",
                &doc.filename,
                doc.content.as_bytes(),
                "discovery_document",
            ),
            note: None,
        })
        .collect();

    let attachments = docs
        .attachments
        .iter()
        .map(|att| {
            let (content, note) = match base64::engine::general_purpose::STANDARD.decode(&att.data)
            {
                Ok(bytes) => (bytes, None),
                Err(e) => (
                    att.data.clone().into_bytes(),
                    Some(format!("not valid base64 ({e}); kept as served")),
                ),
            };
            DiscoveredFile {
                filename: att.filename.clone(),
                title: None,
                content_type: Some(att.content_type.clone()),
                evidence_id: keep(
                    "discovery/attachments",
                    &att.filename,
                    &content,
                    "discovery_attachment",
                ),
                note,
            }
        })
        .collect();

    DiscoveryRecord {
        status: if docs.documents.is_empty() { "empty" } else { "obtained" },
        response_evidence_id,
        documents,
        attachments,
        testable: req_set.testable.len(),
        compliant_by_declaration: req_set.compliant_by_declaration.len(),
        unclassified: req_set.unclassified.len(),
    }
}

/// Parse `StrictDoc` documents from `DocsResponse`.
/// Extracts `[REQUIREMENT]` blocks and classifies them.
pub fn parse_requirements(docs: &DocsResponse) -> RequirementSet {
    let mut testable = Vec::new();
    let mut by_declaration = Vec::new();
    let mut unclassified = Vec::new();

    for doc in &docs.documents {
        for req in parse_document(&doc.content) {
            if req.uid.is_empty() {
                continue;
            }
            if req.is_testable() {
                testable.push(req);
            } else if !req.verification_methods.is_empty() {
                by_declaration.push(req);
            } else {
                unclassified.push(req);
            }
        }
    }

    RequirementSet { testable, compliant_by_declaration: by_declaration, unclassified }
}

/// Parse `[REQUIREMENT]` blocks from a single `StrictDoc` document string.
fn parse_document(content: &str) -> Vec<Requirement> {
    let mut requirements = Vec::new();
    let mut in_requirement = false;
    let mut current = Requirement {
        uid: String::new(),
        title: String::new(),
        status: String::new(),
        verification_methods: Vec::new(),
    };

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "[REQUIREMENT]" || trimmed == "[COMPOSITE_REQUIREMENT]" {
            if in_requirement && !current.uid.is_empty() {
                requirements.push(current.clone());
            }
            current = Requirement {
                uid: String::new(),
                title: String::new(),
                status: String::new(),
                verification_methods: Vec::new(),
            };
            in_requirement = true;
            continue;
        }

        // Block boundary: start of a non-requirement block ends the current requirement.
        if trimmed.starts_with('[') && trimmed.ends_with(']') && in_requirement {
            if !current.uid.is_empty() {
                requirements.push(current.clone());
            }
            current = Requirement {
                uid: String::new(),
                title: String::new(),
                status: String::new(),
                verification_methods: Vec::new(),
            };
            in_requirement = false;
            continue;
        }

        if !in_requirement {
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("UID:") {
            current.uid = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("TITLE:") {
            current.title = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("STATUS:") {
            current.status = val.trim().to_string();
        } else if let Some(val) = trimmed.strip_prefix("VERIFICATION:") {
            current.verification_methods =
                val.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
    }

    if in_requirement && !current.uid.is_empty() {
        requirements.push(current);
    }

    requirements
}

/// Check that every testable requirement has a matching spec.
///
/// Returns `Ok(())` if all testable requirements are covered.
///
/// # Errors
///
/// Returns an error listing all UNCOVERED UIDs when any testable requirement
/// has no matching spec.
pub fn check_coverage(requirement_set: &RequirementSet, spec_uids: &[String]) -> Result<()> {
    let uncovered: Vec<&str> = requirement_set
        .testable
        .iter()
        .filter(|r| !spec_uids.contains(&r.uid))
        .map(|r| r.uid.as_str())
        .collect();
    if !uncovered.is_empty() {
        bail!("UNCOVERED requirements (testable but no matching spec): {}", uncovered.join(", "));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use sqt_types::{DocsResponse, StrictdocDocument};

    use super::*;

    fn make_docs(content: &str) -> DocsResponse {
        DocsResponse {
            documents: vec![StrictdocDocument {
                filename: "test.sdoc".to_string(),
                title: None,
                content: content.to_string(),
            }],
            attachments: vec![],
        }
    }

    const MULTI_REQ_DOC: &str = "\
[DOCUMENT]
TITLE: Test Document

[SECTION]
TITLE: Section 1

[REQUIREMENT]
UID: REQ-001
TITLE: First requirement
STATUS: Active
VERIFICATION: Test

[REQUIREMENT]
UID: REQ-002
TITLE: Second requirement
STATUS: Active
VERIFICATION: Analysis

[REQUIREMENT]
UID: REQ-003
TITLE: Third requirement
STATUS: Active
VERIFICATION: Test, Review

[REQUIREMENT]
UID: REQ-004
TITLE: Unclassified requirement
STATUS: Draft
";

    /// Verifies: SRS-FUNC-018
    #[test]
    fn parse_requirements_multi_doc() {
        let docs = make_docs(MULTI_REQ_DOC);
        let set = parse_requirements(&docs);

        // REQ-001 and REQ-003 are testable (contain "Test")
        assert_eq!(set.testable.len(), 2, "expected 2 testable requirements");
        let testable_uids: Vec<&str> = set.testable.iter().map(|r| r.uid.as_str()).collect();
        assert!(testable_uids.contains(&"REQ-001"));
        assert!(testable_uids.contains(&"REQ-003"));

        // REQ-002 is compliant by declaration (Analysis only)
        assert_eq!(set.compliant_by_declaration.len(), 1);
        assert_eq!(set.compliant_by_declaration[0].uid, "REQ-002");

        // REQ-004 has no VERIFICATION — unclassified
        assert_eq!(set.unclassified.len(), 1);
        assert_eq!(set.unclassified[0].uid, "REQ-004");
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn is_testable_true_for_test_verification() {
        let req = Requirement {
            uid: "REQ-X".to_string(),
            title: String::new(),
            status: String::new(),
            verification_methods: vec!["Test".to_string()],
        };
        assert!(req.is_testable());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn is_testable_case_insensitive() {
        let req = Requirement {
            uid: "REQ-X".to_string(),
            title: String::new(),
            status: String::new(),
            verification_methods: vec!["TEST".to_string()],
        };
        assert!(req.is_testable());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn is_testable_false_for_analysis_verification() {
        let req = Requirement {
            uid: "REQ-X".to_string(),
            title: String::new(),
            status: String::new(),
            verification_methods: vec!["Analysis".to_string()],
        };
        assert!(!req.is_testable());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn check_coverage_passes_when_all_covered() {
        let docs = make_docs(MULTI_REQ_DOC);
        let set = parse_requirements(&docs);
        let spec_uids = vec!["REQ-001".to_string(), "REQ-003".to_string(), "REQ-EXTRA".to_string()];
        assert!(check_coverage(&set, &spec_uids).is_ok());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn check_coverage_fails_with_correct_uids_when_uncovered() {
        let docs = make_docs(MULTI_REQ_DOC);
        let set = parse_requirements(&docs);
        // Only cover REQ-001, leave REQ-003 uncovered
        let spec_uids = vec!["REQ-001".to_string()];
        let err = check_coverage(&set, &spec_uids).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("UNCOVERED"), "error must mention UNCOVERED: {msg}");
        assert!(msg.contains("REQ-003"), "error must list REQ-003: {msg}");
        assert!(!msg.contains("REQ-001"), "covered UID must not appear in error: {msg}");
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn empty_documents_gives_empty_requirement_set() {
        let docs = DocsResponse { documents: vec![], attachments: vec![] };
        let set = parse_requirements(&docs);
        assert!(set.testable.is_empty());
        assert!(set.compliant_by_declaration.is_empty());
        assert!(set.unclassified.is_empty());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn requirement_without_uid_is_skipped() {
        let content = "\
[REQUIREMENT]
TITLE: No UID requirement
STATUS: Active
VERIFICATION: Test
";
        let docs = make_docs(content);
        let set = parse_requirements(&docs);
        assert!(set.testable.is_empty(), "requirement without UID must be skipped");
        assert!(set.unclassified.is_empty());
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn composite_requirement_is_parsed() {
        let content = "\
[COMPOSITE_REQUIREMENT]
UID: CREQ-001
TITLE: Composite
VERIFICATION: Test
";
        let docs = make_docs(content);
        let set = parse_requirements(&docs);
        assert_eq!(set.testable.len(), 1);
        assert_eq!(set.testable[0].uid, "CREQ-001");
    }

    /// Verifies: SRS-FUNC-018
    #[test]
    fn requirement_at_end_of_file_is_captured() {
        // Ensure the final requirement (no trailing block marker) is included.
        let content = "\
[REQUIREMENT]
UID: REQ-LAST
VERIFICATION: Test
";
        let docs = make_docs(content);
        let set = parse_requirements(&docs);
        assert_eq!(set.testable.len(), 1);
        assert_eq!(set.testable[0].uid, "REQ-LAST");
    }
}
