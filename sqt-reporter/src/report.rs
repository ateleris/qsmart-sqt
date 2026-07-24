use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::specs::{RunResult, SpecResult};

/// Schema version for the JSON report format.
///
/// Increment the minor component when new fields are added in a backward-compatible
/// way. Increment the major component for breaking changes.
pub const SCHEMA_VERSION: &str = "1.0";

// ---------------------------------------------------------------------------
// Provenance
// ---------------------------------------------------------------------------

/// Sanitised summary of the config that is safe to include in reports.
///
/// Token values are never recorded — only the env-var *name* (if configured)
/// so that operators can see which secret was in use without leaking it.
#[derive(Debug, Clone, Serialize)]
pub struct ConfigSummary {
    pub runner_url: String,
    /// Name of the bearer-token env var, or `null` when unauthenticated.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
    pub max_concurrent_jobs: u32,
}

/// Provenance information recorded at the start of every qualification run.
///
/// Fields are populated from the `VersionResponse` returned by the Runner's
/// `GET /api/v1/version` endpoint plus the reporter's own `CARGO_PKG_VERSION`.
#[derive(Debug, Clone, Serialize)]
pub struct RunProvenance {
    /// Version of the `sqt-reporter` binary that produced this report.
    pub sqt_version: String,
    /// Version of the SQT Runner binary (from `VersionResponse.runner_version`).
    pub runner_version: String,
    /// Hardware target identifier on the Runner (e.g. `"gr740-leon4"`).
    pub runner_target_id: String,
    /// OS name reported by the Runner (e.g. `"linux"`).
    pub runner_os: String,
    /// Key toolchain versions available on the Runner (e.g. `{"onnxruntime": "1.17"}`).
    pub runner_toolchain_versions: HashMap<String, String>,
    /// Sanitised config summary — no token values.
    pub config_summary: ConfigSummary,
}

// ---------------------------------------------------------------------------
// Outcome kind
// ---------------------------------------------------------------------------

/// Three-state outcome category for a single constraint check.
///
/// * `Pass` — constraint satisfied.
/// * `Fail` — constraint violated (binary-under-test did not meet the requirement).
/// * `InfraError` — the Runner could not execute the job (network failure, internal
///   server error, job timeout on the runner side). Distinct from a constraint `Fail`.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OutcomeKind {
    Pass,
    Fail,
    InfraError,
}

// ---------------------------------------------------------------------------
// Report DTO
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Report {
    /// JSON schema version. Always the first field in the serialized output.
    pub schema_version: &'static str,
    pub timestamp: String,
    pub config_version: String,
    /// `true` when the run was cut short by SIGTERM or Ctrl-C.
    pub interrupted: bool,
    pub provenance: RunProvenance,
    pub summary: Summary,
    pub results: Vec<SpecReport>,
}

#[derive(Serialize)]
pub struct Summary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub infra_errors: usize,
}

impl Summary {
    /// Whether the run should fail the CI gate (non-zero exit).
    ///
    /// A spec that failed a constraint *or* infra-errored is a gate failure
    /// (SRS-OPS-001, SRS-OPS-006). Checking only `failed` would let a run where
    /// every spec is an `INFRA_ERROR` — `failed == 0` — exit 0 (F-C4).
    pub fn is_gate_failure(&self) -> bool {
        self.failed > 0 || self.infra_errors > 0
    }
}

#[derive(Serialize)]
pub struct SpecReport {
    pub requirement_uid: String,
    pub passed: bool,
    pub outcome_kind: OutcomeKind,
    pub target: TargetReport,
    pub outcomes: Vec<OutcomeReport>,
}

#[derive(Serialize)]
pub struct TargetReport {
    pub binary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<String>,
    pub timeout_secs: u64,
    /// Whether all repetitions of the job produced bit-identical outputs.
    ///
    /// `null` (serialized as JSON `null`) when the Runner did not include an
    /// `output` block in the response (e.g. job failed or instrumentation not
    /// configured for output capture).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_consistent: Option<bool>,
}

#[derive(Serialize)]
pub struct OutcomeReport {
    pub metric: String,
    pub operator: String,
    pub outcome_kind: OutcomeKind,
    pub passed: bool,
    pub actual: f64,
    pub expected: f64,
    pub unit: String,
    pub message: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub evidence: HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// Conversion
// ---------------------------------------------------------------------------

impl Report {
    /// Build a `Report` from a completed `RunResult`.
    ///
    /// Provenance must be provided by the caller — it is assembled from the
    /// `VersionResponse` obtained at session start and the active config.
    pub fn from_run_result(result: &RunResult, provenance: RunProvenance) -> Self {
        let results: Vec<SpecReport> = result.results.iter().map(spec_report).collect();
        let total = results.len();
        let passed = results.iter().filter(|r| r.passed).count();
        let infra_errors =
            results.iter().filter(|r| r.outcome_kind == OutcomeKind::InfraError).count();
        let failed = total - passed - infra_errors;
        Self {
            schema_version: SCHEMA_VERSION,
            timestamp: result.timestamp.clone(),
            config_version: result.config_version.clone(),
            interrupted: result.interrupted,
            provenance,
            summary: Summary { total, passed, failed, infra_errors },
            results,
        }
    }
}

fn spec_report(sr: &SpecResult) -> SpecReport {
    use crate::specs::OutcomeKind as SpecOutcomeKind;

    let report_outcome_kind = match sr.outcome {
        SpecOutcomeKind::Pass => OutcomeKind::Pass,
        SpecOutcomeKind::Fail => OutcomeKind::Fail,
        SpecOutcomeKind::InfraError => OutcomeKind::InfraError,
    };

    // For normal Pass/Fail results, zip constraints with outcomes so the
    // metric name and operator are available.  For InfraError results the
    // synthetic outcome carries all relevant information already, so we
    // fall back to iterating `outcomes` directly without zipping constraints.
    let outcomes: Vec<OutcomeReport> = if sr.outcome == SpecOutcomeKind::InfraError {
        sr.outcomes
            .iter()
            .map(|outcome| OutcomeReport {
                metric: "infra".to_owned(),
                operator: "n/a".to_owned(),
                outcome_kind: OutcomeKind::InfraError,
                passed: false,
                actual: f64::NAN,
                expected: f64::NAN,
                unit: String::new(),
                message: outcome.message.clone(),
                evidence: outcome.evidence.clone(),
            })
            .collect()
    } else {
        debug_assert_eq!(
            sr.spec.constraints.len(),
            sr.outcomes.len(),
            "constraint and outcome counts must match — orchestrator bug"
        );
        sr.spec
            .constraints
            .iter()
            .zip(sr.outcomes.iter())
            .map(|(constraint, outcome)| {
                let kind = if outcome.passed { OutcomeKind::Pass } else { OutcomeKind::Fail };
                OutcomeReport {
                    metric: constraint.metric_name().to_owned(),
                    operator: constraint.op().to_string(),
                    outcome_kind: kind,
                    passed: outcome.passed,
                    actual: outcome.actual,
                    expected: outcome.expected,
                    unit: constraint.unit().to_owned(),
                    message: outcome.message.clone(),
                    evidence: outcome.evidence.clone(),
                }
            })
            .collect()
    };

    // Extract result_consistent from evidence (the orchestrator records it there).
    let result_consistent = sr
        .outcomes
        .first()
        .and_then(|o| o.evidence.get("result_consistent"))
        .and_then(|v| v.parse::<bool>().ok());

    SpecReport {
        requirement_uid: sr.spec.requirement_uid.clone(),
        passed: sr.passed,
        outcome_kind: report_outcome_kind,
        target: TargetReport {
            binary: sr.spec.target.binary.to_string_lossy().into_owned(),
            input: sr.spec.target.input.as_ref().map(|p| p.to_string_lossy().into_owned()),
            timeout_secs: sr.spec.target.timeout.as_secs(),
            result_consistent,
        },
        outcomes,
    }
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// Print a human-readable summary to stdout.
pub fn print_terminal(report: &Report) {
    for spec in &report.results {
        let status = match spec.outcome_kind {
            OutcomeKind::Pass => "PASS",
            OutcomeKind::Fail => "FAIL",
            OutcomeKind::InfraError => "[INFRA]",
        };
        println!("{:<60} {status}", spec.requirement_uid);
        for outcome in &spec.outcomes {
            let outcome_label = match outcome.outcome_kind {
                OutcomeKind::Pass => "PASS",
                OutcomeKind::Fail => "FAIL",
                OutcomeKind::InfraError => "[INFRA]",
            };
            println!("    {:<20} {} {}", outcome.metric, outcome_label, outcome.message);

            // For INFRA_ERROR outcomes, surface the error details on the next line.
            if outcome.outcome_kind == OutcomeKind::InfraError {
                let error_code =
                    outcome.evidence.get("error_code").map_or("UNKNOWN", String::as_str);
                let error_message =
                    outcome.evidence.get("error_message").map_or("", String::as_str);
                println!("        error_code={error_code} error_message={error_message}");
                if let Some(details) = outcome.evidence.get("error_details") {
                    println!("        error_details={details}");
                }
            }
        }
        // Show result_consistent in the evidence section.
        if let Some(rc) = spec.target.result_consistent {
            println!("    result_consistent: {rc}");
        }
    }
    println!("{}", "─".repeat(50));
    let interrupted_note = if report.interrupted { " [INTERRUPTED]" } else { "" };
    println!(
        "{}/{} specs passed, {} failed, {} infra errors{interrupted_note}",
        report.summary.passed,
        report.summary.total,
        report.summary.failed,
        report.summary.infra_errors,
    );
}

/// Bundle qualification artefacts into a `qdp.zip` archive.
///
/// Collects the JSON report, traceability files, and the entire `evidence/`
/// directory into a single ZIP archive placed at `output_dir/qdp.zip`.
/// Each path argument is optional — missing or non-existent files are silently
/// skipped so that a partial run still produces a usable bundle.
///
/// # Errors
///
/// Returns an error if the archive cannot be created or any included file
/// cannot be read.
pub fn write_qdp(
    output_dir: &Path,
    report_json_path: Option<&Path>,
    report_pdf_path: Option<&Path>,
    traceability_json: Option<&Path>,
    traceability_txt: Option<&Path>,
    evidence_dir: Option<&Path>,
) -> anyhow::Result<std::path::PathBuf> {
    use std::io::Write as _;

    use anyhow::Context as _;
    use zip::write::SimpleFileOptions;

    let qdp_path = output_dir.join("qdp.zip");
    if let Some(parent) = qdp_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let file = std::fs::File::create(&qdp_path)
        .with_context(|| format!("cannot create QDP archive: {}", qdp_path.display()))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    // Helper closure: add a single file to the archive under `entry_name`.
    let add_file =
        |zip: &mut zip::ZipWriter<_>, entry_name: &str, path: &Path| -> anyhow::Result<()> {
            let data =
                std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
            zip.start_file(entry_name, options)?;
            zip.write_all(&data)?;
            Ok(())
        };

    if let Some(p) = report_json_path
        && p.exists()
    {
        add_file(&mut zip, "report.json", p)?;
    }
    if let Some(p) = report_pdf_path
        && p.exists()
    {
        add_file(&mut zip, "report.pdf", p)?;
    }
    if let Some(p) = traceability_json
        && p.exists()
    {
        add_file(&mut zip, "traceability.json", p)?;
    }
    if let Some(p) = traceability_txt
        && p.exists()
    {
        add_file(&mut zip, "traceability.txt", p)?;
    }

    // Include all evidence files recursively under their relative paths.
    if let Some(ev_dir) = evidence_dir
        && ev_dir.is_dir()
    {
        // Strip the evidence dir's parent so paths in the archive are
        // rooted at "evidence/<file>" rather than absolute.
        let strip_base = ev_dir.parent().unwrap_or(ev_dir);
        for entry in walkdir::WalkDir::new(ev_dir).min_depth(1) {
            let entry = entry?;
            if entry.file_type().is_file() {
                let rel = entry.path().strip_prefix(strip_base)?;
                add_file(&mut zip, &rel.display().to_string(), entry.path())?;
            }
        }
    }

    zip.finish()?;
    tracing::info!("QDP archive written to {}", qdp_path.display());
    Ok(qdp_path)
}

/// Serialize the report as pretty-printed JSON to `path`.
///
/// # Errors
///
/// Returns an error if the file cannot be created or serialization fails.
pub fn write_json(report: &Report, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, report)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// PDF report (simple, unstyled — a richer typst/template engine comes later)
// ---------------------------------------------------------------------------

/// One pre-laid-out text line for the PDF renderer.
struct PdfLine {
    text: String,
    size: f32,
    bold: bool,
}

impl PdfLine {
    fn normal(text: impl Into<String>) -> Self {
        Self { text: text.into(), size: 10.0, bold: false }
    }
    fn bold(text: impl Into<String>, size: f32) -> Self {
        Self { text: text.into(), size, bold: true }
    }
    fn blank() -> Self {
        Self { text: String::new(), size: 10.0, bold: false }
    }
}

/// Truncate a line so it does not run off the page (no word wrapping yet).
fn clip(s: &str) -> String {
    const MAX: usize = 110;
    if s.chars().count() <= MAX {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(MAX - 1).collect();
        out.push('…');
        out
    }
}

/// Flatten a [`Report`] into a top-to-bottom sequence of text lines.
fn report_lines(report: &Report) -> Vec<PdfLine> {
    let s = &report.summary;
    let p = &report.provenance;
    let interrupted = if report.interrupted { "  [INTERRUPTED]" } else { "" };

    let mut lines = vec![
        PdfLine::bold("SQT Qualification Report", 16.0),
        PdfLine::normal(format!("Generated: {}", report.timestamp)),
        PdfLine::normal(format!("Config version: {}", report.config_version)),
        PdfLine::normal(format!(
            "Reporter {} — Runner {} ({}, {})",
            p.sqt_version, p.runner_version, p.runner_target_id, p.runner_os
        )),
        PdfLine::normal(format!("Runner URL: {}", p.config_summary.runner_url)),
        PdfLine::blank(),
        PdfLine::bold(
            format!(
                "Summary: {}/{} passed, {} failed, {} infra errors{interrupted}",
                s.passed, s.total, s.failed, s.infra_errors
            ),
            12.0,
        ),
        PdfLine::blank(),
    ];

    for spec in &report.results {
        let status = match spec.outcome_kind {
            OutcomeKind::Pass => "PASS",
            OutcomeKind::Fail => "FAIL",
            OutcomeKind::InfraError => "INFRA",
        };
        lines.push(PdfLine::bold(format!("{}  [{status}]", spec.requirement_uid), 11.0));
        for outcome in &spec.outcomes {
            lines.push(PdfLine::normal(format!("    {}  {}", outcome.metric, outcome.message)));
        }
        if let Some(rc) = spec.target.result_consistent {
            lines.push(PdfLine::normal(format!("    result_consistent: {rc}")));
        }
        lines.push(PdfLine::blank());
    }

    lines
}

/// Render the report as a simple, unstyled PDF at `path`.
///
/// Uses built-in (non-embedded) Helvetica so no font files are required. Layout
/// is plain top-to-bottom text with naive pagination — styling, tables, and a
/// proper template engine are deferred.
///
/// # Errors
///
/// Returns an error if the document cannot be built or the file cannot be written.
pub fn write_pdf(report: &Report, path: &Path) -> Result<()> {
    use std::io::BufWriter;

    use anyhow::Context as _;
    use printpdf::{BuiltinFont, Mm, PdfDocument};

    // A4 portrait. Margins and cursor are in millimetres (printpdf uses f32).
    const PAGE_W: f32 = 210.0;
    const PAGE_H: f32 = 297.0;
    const LEFT: f32 = 15.0;
    const TOP: f32 = 280.0;
    const BOTTOM: f32 = 15.0;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let (doc, page1, layer1) =
        PdfDocument::new("SQT Qualification Report", Mm(PAGE_W), Mm(PAGE_H), "layer");
    let regular =
        doc.add_builtin_font(BuiltinFont::Helvetica).context("add Helvetica builtin font")?;
    let bold =
        doc.add_builtin_font(BuiltinFont::HelveticaBold).context("add Helvetica-Bold builtin")?;

    let mut layer = doc.get_page(page1).get_layer(layer1);
    let mut y = TOP;

    for line in report_lines(report) {
        let line_height = line.size * 0.5; // ~mm per pt, generous leading
        if y - line_height < BOTTOM {
            let (page, layer_idx) = doc.add_page(Mm(PAGE_W), Mm(PAGE_H), "layer");
            layer = doc.get_page(page).get_layer(layer_idx);
            y = TOP;
        }
        if !line.text.is_empty() {
            let font = if line.bold { &bold } else { &regular };
            layer.use_text(clip(&line.text), line.size, Mm(LEFT), Mm(y), font);
        }
        y -= line_height;
    }

    let file = std::fs::File::create(path)
        .with_context(|| format!("cannot create PDF report: {}", path.display()))?;
    doc.save(&mut BufWriter::new(file)).context("failed to write PDF")?;
    tracing::info!("PDF report written to {}", path.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;
    use crate::constraint::{Constraint, ConstraintOutcome, Op};
    use crate::metrics::{Platform, Target};
    use crate::specs::{RunResult, SpecResult, TestSpec};

    fn make_target(binary: &str) -> Target {
        Target {
            binary: PathBuf::from(binary),
            input: None,
            platform: Platform::Local,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(30),
        }
    }

    fn make_spec(uid: &str, binary: &str) -> TestSpec {
        TestSpec {
            requirement_uid: uid.to_string(),
            target: make_target(binary),
            constraints: vec![Constraint::return_code(Op::Eq, 0)],
            ..Default::default()
        }
    }

    fn make_provenance() -> RunProvenance {
        RunProvenance {
            sqt_version: "0.1.0".to_string(),
            runner_version: "0.2.0".to_string(),
            runner_target_id: "gr740-leon4".to_string(),
            runner_os: "linux".to_string(),
            runner_toolchain_versions: HashMap::new(),
            config_summary: ConfigSummary {
                runner_url: "http://localhost:8080".to_string(),
                token_env: None,
                max_concurrent_jobs: 8,
            },
        }
    }

    fn passing_spec_result(uid: &str, binary: &str) -> SpecResult {
        let spec = make_spec(uid, binary);
        let outcome = ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence: HashMap::new(),
        };
        SpecResult::new(spec, vec![outcome])
    }

    fn failing_spec_result(uid: &str, binary: &str) -> SpecResult {
        let spec = make_spec(uid, binary);
        let outcome = ConstraintOutcome {
            passed: false,
            actual: 1.0,
            expected: 0.0,
            message: "1 == 0: FAIL".to_string(),
            evidence: HashMap::new(),
        };
        SpecResult::new(spec, vec![outcome])
    }

    fn make_run_result(results: Vec<SpecResult>) -> RunResult {
        RunResult {
            results,
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            config_version: "1.0.0".to_string(),
            interrupted: false,
            requirement_set: None,
        }
    }

    #[test]
    fn gate_failure_when_a_spec_failed() {
        let s = Summary { total: 2, passed: 1, failed: 1, infra_errors: 0 };
        assert!(s.is_gate_failure());
    }

    #[test]
    fn gate_failure_when_all_infra_errors_even_with_zero_failed() {
        // The regression this guards (F-C4): a run where every spec is an
        // INFRA_ERROR has failed == 0, but it must still fail the CI gate.
        let s = Summary { total: 3, passed: 0, failed: 0, infra_errors: 3 };
        assert!(s.is_gate_failure());
    }

    #[test]
    fn no_gate_failure_when_all_pass() {
        let s = Summary { total: 2, passed: 2, failed: 0, infra_errors: 0 };
        assert!(!s.is_gate_failure());
    }

    #[test]
    fn no_gate_failure_for_empty_run() {
        let s = Summary { total: 0, passed: 0, failed: 0, infra_errors: 0 };
        assert!(!s.is_gate_failure());
    }

    #[test]
    fn report_from_run_result_computes_summary() {
        let run_result = make_run_result(vec![
            passing_spec_result("REQ-001", "/bin/app"),
            failing_spec_result("REQ-002", "/bin/app"),
        ]);
        let report = Report::from_run_result(&run_result, make_provenance());
        assert_eq!(report.summary.total, 2);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.failed, 1);
        assert_eq!(report.summary.infra_errors, 0);
    }

    #[test]
    fn report_outcome_carries_evidence() {
        let spec = make_spec("REQ-001", "/bin/app");
        let mut evidence = HashMap::new();
        evidence.insert("run_id".to_string(), "abc-123".to_string());
        let outcome = ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence,
        };
        let sr = SpecResult::new(spec, vec![outcome]);
        let run_result = make_run_result(vec![sr]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let ev = &report.results[0].outcomes[0].evidence;
        assert_eq!(ev.get("run_id").map(String::as_str), Some("abc-123"));
    }

    #[test]
    fn report_serializes_to_json() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["summary"]["total"], 1);
        assert_eq!(parsed["summary"]["passed"], 1);
        assert_eq!(parsed["config_version"], "1.0.0");
        assert_eq!(parsed["results"][0]["requirement_uid"], "REQ-001");
    }

    #[test]
    fn write_json_creates_parent_directory() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/subdir/report.json");
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result, make_provenance());
        write_json(&report, &path).expect("write_json must create parent dirs");
        assert!(path.exists(), "report file must be created");
    }

    #[test]
    fn report_empty_results() {
        let run_result = make_run_result(vec![]);
        let report = Report::from_run_result(&run_result, make_provenance());
        assert_eq!(report.summary.total, 0);
        assert_eq!(report.summary.passed, 0);
        assert_eq!(report.summary.failed, 0);
        assert_eq!(report.summary.infra_errors, 0);
        assert!(report.results.is_empty());
    }

    // -------------------------------------------------------------------------
    // New hardening tests (T-075)
    // -------------------------------------------------------------------------

    #[test]
    fn report_schema_version_is_present_in_json() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["schema_version"], "1.0", "schema_version must be present and correct");
    }

    #[test]
    fn report_schema_version_is_first_json_field() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let json = serde_json::to_string(&report).expect("serialize");
        // The first key in the JSON object must be "schema_version".
        let first_key = json
            .trim_start_matches('{')
            .trim_start()
            .split(':')
            .next()
            .map(|s| s.trim().trim_matches('"'));
        assert_eq!(
            first_key,
            Some("schema_version"),
            "schema_version must be the first JSON field"
        );
    }

    #[test]
    fn infra_error_serializes_with_infra_error_outcome_kind() {
        use crate::specs::SpecResult;
        let spec = make_spec("REQ-INFRA", "/bin/app");
        let mut evidence = HashMap::new();
        evidence.insert("error_code".to_string(), "RUNNER_TIMEOUT".to_string());
        evidence.insert("error_message".to_string(), "job timed out".to_string());
        let sr = SpecResult::infra_error(spec, "job timed out".to_string(), evidence);
        let run_result = make_run_result(vec![sr]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(
            parsed["results"][0]["outcome_kind"], "INFRA_ERROR",
            "InfraError spec must serialize as INFRA_ERROR"
        );
        assert_eq!(
            parsed["results"][0]["outcomes"][0]["outcome_kind"], "INFRA_ERROR",
            "InfraError outcome must also have INFRA_ERROR kind"
        );
        assert_eq!(parsed["summary"]["infra_errors"], 1);
        assert_eq!(parsed["summary"]["failed"], 0);
    }

    #[test]
    fn result_consistent_propagates_from_evidence() {
        let spec = make_spec("REQ-001", "/bin/app");
        let mut evidence = HashMap::new();
        evidence.insert("result_consistent".to_string(), "true".to_string());
        evidence.insert("job_id".to_string(), "job-123".to_string());
        let outcome = ConstraintOutcome {
            passed: true,
            actual: 0.0,
            expected: 0.0,
            message: "0 == 0: PASS".to_string(),
            evidence,
        };
        let sr = SpecResult::new(spec, vec![outcome]);
        let run_result = make_run_result(vec![sr]);
        let report = Report::from_run_result(&run_result, make_provenance());
        assert_eq!(
            report.results[0].target.result_consistent,
            Some(true),
            "result_consistent must be extracted from evidence"
        );
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(
            parsed["results"][0]["target"]["result_consistent"], true,
            "result_consistent must appear in JSON"
        );
    }

    #[test]
    fn write_qdp_bundles_artefacts() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path();

        // Write dummy artefact files.
        let report_path = out.join("report.json");
        std::fs::write(&report_path, b"{\"schema_version\":\"1.0\"}").unwrap();
        let t_json = out.join("traceability.json");
        std::fs::write(&t_json, b"{}").unwrap();
        let t_txt = out.join("traceability.txt");
        std::fs::write(&t_txt, b"ok").unwrap();

        // Create a small evidence dir with one file.
        let ev_dir = out.join("evidence");
        std::fs::create_dir_all(&ev_dir).unwrap();
        std::fs::write(ev_dir.join("run-001.json"), b"{\"job\":\"run-001\"}").unwrap();

        let qdp =
            write_qdp(out, Some(&report_path), None, Some(&t_json), Some(&t_txt), Some(&ev_dir))
                .expect("write_qdp must succeed");

        assert!(qdp.exists(), "qdp.zip must be created");
        // Verify the archive contains the expected entries.
        let zip_file = std::fs::File::open(&qdp).unwrap();
        let mut archive = zip::ZipArchive::new(zip_file).unwrap();
        let names: Vec<String> =
            (0..archive.len()).map(|i| archive.by_index(i).unwrap().name().to_owned()).collect();
        assert!(names.contains(&"report.json".to_owned()), "must contain report.json");
        assert!(names.contains(&"traceability.json".to_owned()), "must contain traceability.json");
        assert!(names.contains(&"traceability.txt".to_owned()), "must contain traceability.txt");
        assert!(
            names.iter().any(|n| n.ends_with("run-001.json")),
            "must contain evidence file; got {names:?}"
        );
    }

    #[test]
    fn write_pdf_produces_a_valid_pdf_file() {
        let result = make_run_result(vec![
            passing_spec_result("REQ-001", "build/app"),
            failing_spec_result("REQ-002", "build/app"),
        ]);
        let report = Report::from_run_result(&result, make_provenance());

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.pdf");
        write_pdf(&report, &path).expect("write_pdf must succeed");

        assert!(path.exists(), "report.pdf must be created");
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 200, "PDF should be non-trivial; got {} bytes", bytes.len());
        assert!(bytes.starts_with(b"%PDF-"), "file must start with the PDF magic header");
    }

    #[test]
    fn result_consistent_null_when_absent_from_evidence() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let report = Report::from_run_result(&run_result, make_provenance());
        assert_eq!(
            report.results[0].target.result_consistent, None,
            "result_consistent must be None when not in evidence"
        );
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        // With skip_serializing_if = "Option::is_none", the field is absent when None.
        assert!(
            parsed["results"][0]["target"]["result_consistent"].is_null()
                || parsed["results"][0]["target"].get("result_consistent").is_none(),
            "result_consistent must be null or absent when None"
        );
    }

    #[test]
    fn interrupted_flag_propagates_to_report() {
        let mut run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        run_result.interrupted = true;
        let report = Report::from_run_result(&run_result, make_provenance());
        assert!(report.interrupted, "interrupted flag must propagate to Report");
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["interrupted"], true);
    }

    #[test]
    fn provenance_fields_appear_in_json() {
        let run_result = make_run_result(vec![passing_spec_result("REQ-001", "/bin/app")]);
        let mut provenance = make_provenance();
        provenance.sqt_version = "0.5.0".to_string();
        provenance.runner_version = "1.2.3".to_string();
        provenance.runner_target_id = "test-target".to_string();
        let report = Report::from_run_result(&run_result, provenance);
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["provenance"]["sqt_version"], "0.5.0");
        assert_eq!(parsed["provenance"]["runner_version"], "1.2.3");
        assert_eq!(parsed["provenance"]["runner_target_id"], "test-target");
    }

    #[test]
    fn outcome_kind_pass_fail_serialize_correctly() {
        let run_result = make_run_result(vec![
            passing_spec_result("REQ-001", "/bin/app"),
            failing_spec_result("REQ-002", "/bin/app"),
        ]);
        let report = Report::from_run_result(&run_result, make_provenance());
        let json = serde_json::to_string(&report).expect("serialize");
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("parse");
        assert_eq!(parsed["results"][0]["outcome_kind"], "PASS");
        assert_eq!(parsed["results"][1]["outcome_kind"], "FAIL");
        assert_eq!(parsed["results"][0]["outcomes"][0]["outcome_kind"], "PASS");
        assert_eq!(parsed["results"][1]["outcomes"][0]["outcome_kind"], "FAIL");
    }
}
