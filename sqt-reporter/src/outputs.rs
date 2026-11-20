// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Everything a qualification run writes after its jobs are evaluated: the
//! persisted outcomes, the evidence manifest, the run report, the PDF, the
//! traceability matrix and the QDP archive, all inside the run's directory
//! (SRS-FUNC-008, SRS-FUNC-009).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::{
    evidence::{EvidenceMeta, EvidenceStore},
    report::{self, Report, RunProvenance},
    specs::RunResult,
    traceability,
};

/// File name of the `n`-th specification's outcomes, e.g. `001-REQ-X.json`.
/// A specification expanded per workload type (SRS-FUNC-019) also names its
/// workload type, `001-REQ-X-regression.json`, so the six files of one
/// requirement can be told apart without opening them.
fn outcome_file_name(n: usize, uid: &str, fan_out_workload: Option<&str>) -> String {
    match fan_out_workload {
        Some(workload) => format!("{n:03}-{uid}-{workload}.json"),
        None => format!("{n:03}-{uid}.json"),
    }
}

/// Where the outputs of a run were written.
#[derive(Debug)]
pub struct RunOutputs {
    /// The run directory.
    pub run_dir: PathBuf,
    /// The JSON report in the run directory.
    pub report_json: PathBuf,
    /// The QDP archive of the run.
    pub qdp: PathBuf,
}

/// Persist each specification's constraint outcomes as evidence, write the
/// manifest, then write the report and the derived artefacts into the run
/// directory. `report_copy`, when given, receives a copy of `report.json`.
///
/// # Errors
///
/// Returns an error if any output cannot be written.
pub fn write_run_outputs(
    result: &mut RunResult,
    provenance: RunProvenance,
    store: &mut EvidenceStore,
    report_copy: Option<&Path>,
) -> Result<(Report, RunOutputs)> {
    for (i, spec_result) in result.results.iter_mut().enumerate() {
        let uid = spec_result.spec.requirement_uid.clone();
        let workload = report::workload_name(&spec_result.spec.workload_type);
        let outcomes = report::spec_report(spec_result);
        let mut details = std::collections::BTreeMap::new();
        details.insert("workload_type".to_owned(), workload.to_owned());
        let id = store.store_json(
            "outcomes",
            &outcome_file_name(i + 1, &uid, spec_result.spec.fan_out.then_some(workload)),
            &outcomes,
            EvidenceMeta {
                kind: "constraint_outcomes",
                requirement_uid: Some(uid),
                details,
                ..EvidenceMeta::default()
            },
        )?;
        spec_result.evidence.push(id);
    }
    let manifest = store.write_manifest()?;

    let run_dir = store.run().path.clone();
    let mut rep = Report::from_run_result(result, provenance);
    rep.run_id = Some(store.run().id.clone());
    rep.evidence_manifest = Some(manifest.to_owned());

    let report_json = run_dir.join("report.json");
    report::write_json(&rep, &report_json)?;
    if let Some(copy) = report_copy {
        if let Some(parent) = copy.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&report_json, copy)
            .with_context(|| format!("cannot copy report to {}", copy.display()))?;
    }
    report::write_pdf(&rep, &run_dir.join("report.pdf"))?;

    if let Some(ref req_set) = result.requirement_set {
        let matrix = traceability::build(req_set, result, &rep.timestamp);
        traceability::write_json(&matrix, &run_dir.join("traceability.json"))?;
        traceability::write_text(&matrix, &run_dir.join("traceability.txt"))?;
    }

    let qdp = report::write_qdp(&run_dir)?;
    Ok((rep, RunOutputs { run_dir, report_json, qdp }))
}

#[cfg(test)]
mod tests {
    use super::outcome_file_name;

    /// Verifies: SRS-FUNC-008, SRS-FUNC-019
    #[test]
    fn fanned_out_outcome_files_name_their_workload_type() {
        assert_eq!(outcome_file_name(1, "REQ-X", None), "001-REQ-X.json");
        assert_eq!(outcome_file_name(4, "REQ-X", Some("regression")), "004-REQ-X-regression.json");
    }
}
