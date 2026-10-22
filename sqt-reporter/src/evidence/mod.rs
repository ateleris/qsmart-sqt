// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Per-run output directory and evidence store (SRS-FUNC-008, SRS-OPS-004).
//!
//! Every qualification run writes into its own directory:
//!
//! ```text
//! <report.output_dir>/runs/<run_id>/
//!     report.json            run report (SRS-FUNC-009)
//!     report.pdf, traceability.json, traceability.txt, qdp.zip
//!     evidence/
//!         manifest.json      every evidence artefact: id, kind, SHA-256, size
//!         runner/…           raw Runner responses, in arrival order
//!         outcomes/…         per-specification constraint outcomes
//! ```
//!
//! An evidence artefact's stable identifier is its path relative to the run
//! directory, e.g. `evidence/runner/0003-submit_job-job-1.json`.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Path of the manifest, relative to the run directory.
pub const MANIFEST_ID: &str = "evidence/manifest.json";

// ---------------------------------------------------------------------------
// Run directory
// ---------------------------------------------------------------------------

/// The directory reserved for one qualification run.
#[derive(Debug, Clone)]
pub struct RunDir {
    /// Sortable UTC timestamp, e.g. `20261022T101530123Z`: lexical order is
    /// chronological order (SRS-OPS-004).
    pub id: String,
    pub path: PathBuf,
}

impl RunDir {
    /// Create a new, empty run directory under `<output_dir>/runs/`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn create(output_dir: &Path) -> Result<Self> {
        let runs = output_dir.join("runs");
        std::fs::create_dir_all(&runs)
            .with_context(|| format!("cannot create run directory parent: {}", runs.display()))?;
        let mut now = chrono::Utc::now();
        loop {
            let id = now.format("%Y%m%dT%H%M%S%3fZ").to_string();
            let path = runs.join(&id);
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { id, path }),
                // Two runs within the same millisecond: take the next one.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    now += chrono::Duration::milliseconds(1);
                }
                Err(e) => {
                    return Err(e).with_context(|| {
                        format!("cannot create run directory {}", path.display())
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Evidence store
// ---------------------------------------------------------------------------

/// One persisted evidence artefact, as listed in the manifest.
#[derive(Debug, Clone, Serialize)]
pub struct EvidenceEntry {
    /// Stable identifier: the path relative to the run directory.
    pub id: String,
    /// `runner_response` or `constraint_outcomes`.
    pub kind: String,
    pub sha256: String,
    pub size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requirement_uid: Option<String>,
    /// Further facts about the artefact, e.g. the HTTP status of a response.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub details: BTreeMap<String, String>,
}

/// What an artefact is and what it belongs to.
#[derive(Debug, Clone, Default)]
pub struct EvidenceMeta {
    pub kind: &'static str,
    pub job_id: Option<String>,
    pub requirement_uid: Option<String>,
    pub details: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct Manifest<'a> {
    run_id: &'a str,
    entries: &'a [EvidenceEntry],
}

pub struct EvidenceStore {
    run: RunDir,
    entries: Vec<EvidenceEntry>,
}

impl EvidenceStore {
    /// Create a store writing into `<run>/evidence/`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new(run: RunDir) -> Result<Self> {
        let dir = run.path.join("evidence");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("cannot create evidence dir: {}", dir.display()))?;
        Ok(Self { run, entries: Vec::new() })
    }

    pub fn run(&self) -> &RunDir {
        &self.run
    }

    /// Write `content` to `evidence/<folder>/<file_name>` and list it in the
    /// manifest. `file_name` is sanitised: it may come from the Runner (a job
    /// id), so it can never leave the folder. Returns the artefact's id.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or file cannot be created/written.
    pub fn store(
        &mut self,
        folder: &str,
        file_name: &str,
        content: &[u8],
        meta: EvidenceMeta,
    ) -> Result<String> {
        let id = format!("evidence/{folder}/{}", sanitize_file_name(file_name));
        let path = self.run.path.join(&id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write evidence file: {}", path.display()))?;
        self.entries.push(EvidenceEntry {
            id: id.clone(),
            kind: meta.kind.to_owned(),
            sha256: sha256_hex(content),
            size_bytes: content.len() as u64,
            job_id: meta.job_id,
            requirement_uid: meta.requirement_uid,
            details: meta.details,
        });
        Ok(id)
    }

    /// Write a JSON-serialised value; see [`Self::store`].
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialisation fails or the underlying [`Self::store`] call fails.
    pub fn store_json<T: Serialize>(
        &mut self,
        folder: &str,
        file_name: &str,
        value: &T,
        meta: EvidenceMeta,
    ) -> Result<String> {
        let json = serde_json::to_vec_pretty(value).context("cannot serialise evidence to JSON")?;
        self.store(folder, file_name, &json, meta)
    }

    /// All stored evidence entries, in the order they were stored.
    pub fn manifest(&self) -> &[EvidenceEntry] {
        &self.entries
    }

    /// Write `evidence/manifest.json` and return its id ([`MANIFEST_ID`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the manifest cannot be serialised or written.
    pub fn write_manifest(&self) -> Result<&'static str> {
        let path = self.run.path.join(MANIFEST_ID);
        let manifest = Manifest { run_id: &self.run.id, entries: &self.entries };
        let json = serde_json::to_vec_pretty(&manifest).context("cannot serialise manifest")?;
        std::fs::write(&path, json)
            .with_context(|| format!("cannot write evidence manifest: {}", path.display()))?;
        Ok(MANIFEST_ID)
    }
}

/// Keep ASCII letters, digits, `.`, `-` and `_`; replace everything else with
/// `_`, and never return an empty name or one made only of dots.
fn sanitize_file_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' })
        .collect();
    if cleaned.chars().all(|c| c == '.') { format!("_{cleaned}") } else { cleaned }
}

fn sha256_hex(content: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(content).iter().fold(String::with_capacity(64), |mut hex, b| {
        let _ = write!(hex, "{b:02x}");
        hex
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn store_in(tmp: &tempfile::TempDir) -> EvidenceStore {
        EvidenceStore::new(RunDir::create(tmp.path()).unwrap()).unwrap()
    }

    fn meta(kind: &'static str) -> EvidenceMeta {
        EvidenceMeta { kind, ..EvidenceMeta::default() }
    }

    /// Verifies: SRS-FUNC-008, SRS-OPS-004
    #[test]
    fn run_dirs_are_unique_and_chronologically_ordered() {
        let tmp = tempfile::tempdir().unwrap();
        let ids: Vec<String> = (0..5).map(|_| RunDir::create(tmp.path()).unwrap().id).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids, sorted, "ids must be distinct and already in lexical order");
        for id in &ids {
            assert!(tmp.path().join("runs").join(id).is_dir());
            assert_eq!(id.len(), "20261022T101530123Z".len(), "{id}");
        }
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn store_writes_file_under_run_and_returns_relative_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ev = store_in(&tmp);

        let id =
            ev.store("runner", "0001-get_version.json", b"{}", meta("runner_response")).unwrap();

        assert_eq!(id, "evidence/runner/0001-get_version.json");
        assert_eq!(fs::read(ev.run().path.join(&id)).unwrap(), b"{}");
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn file_names_from_the_runner_cannot_escape_the_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ev = store_in(&tmp);

        let id = ev.store("runner", "../../etc/passwd", b"x", meta("runner_response")).unwrap();
        assert_eq!(id, "evidence/runner/.._.._etc_passwd");
        assert!(ev.run().path.join(&id).starts_with(ev.run().path.join("evidence/runner")));

        let id = ev.store("runner", "..", b"x", meta("runner_response")).unwrap();
        assert_eq!(id, "evidence/runner/_..");
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn manifest_lists_entries_with_sha256() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ev = store_in(&tmp);
        let mut details = BTreeMap::new();
        details.insert("http_status".to_owned(), "200".to_owned());
        ev.store(
            "runner",
            "0001-get_job_status-job-1.json",
            b"abc",
            EvidenceMeta {
                kind: "runner_response",
                job_id: Some("job-1".into()),
                requirement_uid: None,
                details,
            },
        )
        .unwrap();
        ev.store_json(
            "outcomes",
            "REQ-1.json",
            &serde_json::json!({"ok": true}),
            meta("constraint_outcomes"),
        )
        .unwrap();

        assert_eq!(ev.write_manifest().unwrap(), MANIFEST_ID);
        let manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(ev.run().path.join(MANIFEST_ID)).unwrap()).unwrap();
        assert_eq!(manifest["run_id"], ev.run().id.as_str());
        let entries = manifest["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0]["job_id"], "job-1");
        assert_eq!(entries[0]["details"]["http_status"], "200");
        assert_eq!(entries[0]["size_bytes"], 3);
        // SHA-256 of "abc" (FIPS 180-2 test vector).
        assert_eq!(
            entries[0]["sha256"],
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(entries[1]["kind"], "constraint_outcomes");
    }
}
