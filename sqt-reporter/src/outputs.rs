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

/// Where the outputs of a run were written.
#[derive(Debug)]
pub struct RunOutputs {
    pub run_dir: PathBuf,
    pub report_json: PathBuf,
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
        let outcomes = report::spec_report(spec_result);
        let id = store.store_json(
            "outcomes",
            &format!("{:03}-{uid}.json", i + 1),
            &outcomes,
            EvidenceMeta {
                kind: "constraint_outcomes",
                requirement_uid: Some(uid),
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
