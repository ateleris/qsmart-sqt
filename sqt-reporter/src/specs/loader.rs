//! YAML-based spec loader.
//!
//! Reads spec files listed explicitly in `config.test_specs.files`.
//! Auto-discovery from the filesystem is prohibited (SRS-MAINT-002).

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use sqt_types::WorkloadType;

use crate::constraint::{Constraint, Op};
use crate::metrics::Target;
use crate::specs::TestSpec;

// ---------------------------------------------------------------------------
// Raw deserialization types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct RawConstraint {
    metric: String,
    operator: String,
    /// Can be a YAML string like `"100ms"` or a plain number like `0`.
    value: serde_yaml::Value,
}

#[derive(Deserialize)]
struct RawTestSpec {
    requirement_uid: String,
    use_case: String,
    input: Option<PathBuf>,
    // repetitions is deserialized for forward compatibility; the orchestrator
    // reads it from the spec and passes it to JobRequest.  It is not used here.
    #[serde(default)]
    #[allow(dead_code)]
    repetitions: Option<u32>,
    #[serde(default)]
    timeout_secs: Option<u64>,
    constraints: Vec<RawConstraint>,
    /// Reference outputs for client-side ml.* accuracy evaluation (T-054).
    #[serde(default)]
    reference_outputs: Option<Vec<serde_json::Value>>,
    /// Reference outputs for numerical precision comparison (SRS-FUNC-016).
    #[serde(default)]
    precision_reference: Option<Vec<serde_json::Value>>,
    /// Absolute tolerance for `ml.output_within_tolerance` (SRS-FUNC-016).
    #[serde(default)]
    precision_tolerance: Option<f64>,
}

// ---------------------------------------------------------------------------
// WorkloadType parsing
// ---------------------------------------------------------------------------

fn parse_use_case(s: &str) -> anyhow::Result<WorkloadType> {
    match s {
        "object_detection" => Ok(WorkloadType::ObjectDetection),
        "image_classification" => Ok(WorkloadType::ImageClassification),
        "pixel_segmentation" => Ok(WorkloadType::PixelSegmentation),
        "classification" => Ok(WorkloadType::Classification),
        "regression" => Ok(WorkloadType::Regression),
        "time_series_forecasting" => Ok(WorkloadType::TimeSeriesForecasting),
        _ => bail!(
            "unknown use_case '{s}'; valid values: object_detection, image_classification, \
             pixel_segmentation, classification, regression, time_series_forecasting"
        ),
    }
}

// ---------------------------------------------------------------------------
// Operator parsing
// ---------------------------------------------------------------------------

fn parse_operator(s: &str) -> anyhow::Result<Op> {
    match s {
        "lte" => Ok(Op::Lte),
        "gte" => Ok(Op::Gte),
        "lt" => Ok(Op::Lt),
        "gt" => Ok(Op::Gt),
        "eq" => Ok(Op::Eq),
        "ne" => Ok(Op::Ne),
        _ => bail!("unknown operator '{s}'; valid values: lte, gte, lt, gt, eq, ne"),
    }
}

// ---------------------------------------------------------------------------
// Unit-aware value parsing
// ---------------------------------------------------------------------------

/// Parse a YAML value (string or number) into a canonical `f64` for the given
/// metric.
///
/// Canonical units:
/// - `time.*` metrics → milliseconds
/// - `memory.*` metrics → kilobytes
/// - Everything else → parse as-is (no unit conversion)
fn parse_value(raw: &serde_yaml::Value, metric: &str) -> anyhow::Result<f64> {
    match raw {
        serde_yaml::Value::Number(n) => {
            let v = n
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("invalid numeric value for metric '{metric}'"))?;
            Ok(v)
        }
        serde_yaml::Value::String(s) => parse_unit_value(s.trim(), metric),
        serde_yaml::Value::Bool(b) => {
            // Support `true`/`false` for boolean-like metrics (e.g., output.result_consistent)
            Ok(if *b { 1.0 } else { 0.0 })
        }
        _ => bail!("unsupported value type for metric '{metric}': {raw:?}"),
    }
}

/// Parse a string like `"100ms"`, `"1.5s"`, `"512KB"`, `"2 MB"`, `"0"` into
/// a canonical `f64` for the given metric.
fn parse_unit_value(s: &str, metric: &str) -> anyhow::Result<f64> {
    // Try bare number first (no suffix)
    if let Ok(v) = s.parse::<f64>() {
        return Ok(v);
    }

    let (num_part, unit_part) = split_number_and_unit(s)?;
    let num: f64 = num_part
        .parse()
        .with_context(|| format!("invalid numeric part '{num_part}' in value '{s}'"))?;

    let unit_lower = unit_part.to_ascii_lowercase();

    if metric.starts_with("time.") || metric.starts_with("time_") {
        // Canonical unit: milliseconds
        match unit_lower.as_str() {
            "ms" | "millisecond" | "milliseconds" => Ok(num),
            "s" | "sec" | "second" | "seconds" => Ok(num * 1_000.0),
            "m" | "min" | "minute" | "minutes" => Ok(num * 60_000.0),
            _ => bail!("unknown time unit '{unit_part}' in '{s}' for metric '{metric}'"),
        }
    } else if metric.starts_with("memory.") || metric.starts_with("memory_") {
        // Canonical unit: kilobytes
        match unit_lower.as_str() {
            "b" | "byte" | "bytes" => Ok(num / 1_024.0),
            "kb" | "kib" | "kilobyte" | "kilobytes" => Ok(num),
            "mb" | "mib" | "megabyte" | "megabytes" => Ok(num * 1_024.0),
            "gb" | "gib" | "gigabyte" | "gigabytes" => Ok(num * 1_024.0 * 1_024.0),
            _ => bail!("unknown memory unit '{unit_part}' in '{s}' for metric '{metric}'"),
        }
    } else {
        // No unit conversion — parse the numeric part as-is and ignore the
        // suffix (or error if a suffix is present for metrics that don't
        // expect one).
        Ok(num)
    }
}

/// Split a string like `"100ms"` or `"512 KB"` into `("100", "ms")`.
///
/// Trims whitespace between number and unit.
fn split_number_and_unit(s: &str) -> anyhow::Result<(&str, &str)> {
    // Find where the numeric part (digits, dot, sign) ends.
    let split_idx =
        s.find(|c: char| !c.is_ascii_digit() && c != '.' && c != '-' && c != '+').ok_or_else(
            || anyhow::anyhow!("no unit suffix found in '{s}'; expected e.g. '100ms' or '512KB'"),
        )?;

    let num_part = s[..split_idx].trim();
    let unit_part = s[split_idx..].trim();

    if num_part.is_empty() {
        bail!("missing numeric part in value '{s}'");
    }
    if unit_part.is_empty() {
        bail!("missing unit part in value '{s}'");
    }

    Ok((num_part, unit_part))
}

// ---------------------------------------------------------------------------
// Conversion: RawTestSpec → TestSpec
// ---------------------------------------------------------------------------

fn convert_spec(raw: RawTestSpec, default_timeout_secs: u64) -> anyhow::Result<TestSpec> {
    let uid = &raw.requirement_uid;
    let workload_type = parse_use_case(&raw.use_case)
        .with_context(|| format!("invalid use_case in spec '{uid}'"))?;

    let timeout_secs = raw.timeout_secs.unwrap_or(default_timeout_secs);

    let target = Target {
        binary: PathBuf::new(), // to be filled in by orchestrator
        input: raw.input,
        timeout: Duration::from_secs(timeout_secs),
        ..Target::default()
    };

    let mut constraints = Vec::with_capacity(raw.constraints.len());
    for (i, rc) in raw.constraints.into_iter().enumerate() {
        let op = parse_operator(&rc.operator)
            .with_context(|| format!("constraint[{i}].operator in spec '{uid}'"))?;

        let value = parse_value(&rc.value, &rc.metric)
            .with_context(|| format!("constraint[{i}].value in spec '{uid}'"))?;

        let metric = &rc.metric;
        let constraint = Constraint::from_name(metric, op, value).ok_or_else(|| {
            anyhow::anyhow!("unknown metric '{metric}' in constraint[{i}] of spec '{uid}'")
        })?;
        constraints.push(constraint);
    }

    Ok(TestSpec {
        requirement_uid: raw.requirement_uid,
        workload_type,
        target,
        constraints,
        reference_outputs: raw.reference_outputs,
        precision_reference: raw.precision_reference,
        precision_tolerance: raw.precision_tolerance,
    })
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Load test specs from an explicit list of YAML files.
///
/// For each file:
/// - Read and deserialise as `RawTestSpec`.
/// - Convert to [`TestSpec`], applying `default_timeout_secs` when the spec
///   does not set its own `timeout_secs`.
///
/// Returns the full list of specs, or an `Err` on the first parse failure
/// (the error message includes the file path).
///
/// # Errors
///
/// Returns an error if any file cannot be read, fails YAML deserialization,
/// contains an unknown `use_case`, an unknown operator, an unknown metric
/// name, or a value with an unrecognised unit suffix.
pub fn load_specs(files: &[PathBuf], default_timeout_secs: u64) -> anyhow::Result<Vec<TestSpec>> {
    let mut specs = Vec::with_capacity(files.len());
    for path in files {
        let spec = load_one_spec(path, default_timeout_secs)
            .with_context(|| format!("failed to load spec file '{}'", path.display()))?;
        specs.push(spec);
    }
    Ok(specs)
}

fn load_one_spec(path: &Path, default_timeout_secs: u64) -> anyhow::Result<TestSpec> {
    let content =
        std::fs::read_to_string(path).with_context(|| format!("reading '{}'", path.display()))?;
    let raw: RawTestSpec = serde_yaml::from_str(&content)
        .with_context(|| format!("parsing YAML in '{}'", path.display()))?;
    convert_spec(raw, default_timeout_secs)
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::Op;

    /// Deserialise a YAML string directly (without file I/O) and convert to `TestSpec`.
    fn parse_yaml(yaml: &str) -> anyhow::Result<TestSpec> {
        let raw: RawTestSpec = serde_yaml::from_str(yaml).with_context(|| "parsing test YAML")?;
        convert_spec(raw, 300)
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn parse_minimal_spec() {
        let yaml = r"
requirement_uid: REQ-ML-001
use_case: image_classification
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let spec = parse_yaml(yaml).expect("minimal spec should parse");
        assert_eq!(spec.requirement_uid, "REQ-ML-001");
        assert_eq!(spec.workload_type, WorkloadType::ImageClassification);
        assert!(spec.target.input.is_none());
        assert_eq!(spec.target.timeout, Duration::from_mins(5));
        assert_eq!(spec.constraints.len(), 1);
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn parse_full_spec_with_all_fields() {
        let yaml = r#"
requirement_uid: REQ-ML-002
use_case: regression
input: datasets/reference.bin
repetitions: 5
timeout_secs: 120
constraints:
  - metric: time.avg_ms
    operator: lte
    value: "100ms"
  - metric: memory.peak_rss_kb
    operator: lte
    value: "512 KB"
  - metric: return_code
    operator: eq
    value: 0
"#;
        let spec = parse_yaml(yaml).expect("full spec should parse");
        assert_eq!(spec.requirement_uid, "REQ-ML-002");
        assert_eq!(spec.workload_type, WorkloadType::Regression);
        assert_eq!(spec.target.input, Some(PathBuf::from("datasets/reference.bin")));
        assert_eq!(spec.target.timeout, Duration::from_mins(2));
        assert_eq!(spec.constraints.len(), 3);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn parse_unit_suffixed_ms_value() {
        let yaml = r#"
requirement_uid: REQ-T-001
use_case: classification
constraints:
  - metric: time.avg_ms
    operator: lte
    value: "100ms"
"#;
        let spec = parse_yaml(yaml).expect("should parse");
        let c = &spec.constraints[0];
        assert_eq!(c.metric_name(), "time.avg_ms");
        // "100ms" → 100.0 ms (no conversion needed)
        assert!((c.expected() - 100.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn parse_unit_suffixed_s_value() {
        let yaml = r#"
requirement_uid: REQ-T-002
use_case: classification
constraints:
  - metric: time.avg_ms
    operator: lte
    value: "1.5s"
"#;
        let spec = parse_yaml(yaml).expect("should parse");
        let c = &spec.constraints[0];
        // "1.5s" → 1500.0 ms
        assert!((c.expected() - 1_500.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn parse_unit_suffixed_kb_value() {
        let yaml = r#"
requirement_uid: REQ-M-001
use_case: classification
constraints:
  - metric: memory.peak_rss_kb
    operator: lte
    value: "512 KB"
"#;
        let spec = parse_yaml(yaml).expect("should parse");
        let c = &spec.constraints[0];
        // "512 KB" → 512.0 KB (no conversion needed)
        assert!((c.expected() - 512.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn parse_unit_suffixed_mb_value() {
        let yaml = r#"
requirement_uid: REQ-M-002
use_case: classification
constraints:
  - metric: memory.peak_rss_kb
    operator: lte
    value: "2 MB"
"#;
        let spec = parse_yaml(yaml).expect("should parse");
        let c = &spec.constraints[0];
        // "2 MB" → 2048.0 KB
        assert!((c.expected() - 2_048.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn missing_required_field_returns_error() {
        // Missing `use_case` — serde should reject this.
        let yaml = r"
requirement_uid: REQ-MISSING
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let result = parse_yaml(yaml);
        assert!(result.is_err(), "missing use_case must be an error");
    }

    /// Verifies: SRS-FUNC-003, SRS-FUNC-019
    #[test]
    fn unknown_use_case_returns_error() {
        let yaml = r"
requirement_uid: REQ-BAD-UC
use_case: depth_estimation
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let result = parse_yaml(yaml);
        assert!(result.is_err(), "unknown use_case must be an error");
        // Use {:#} to get the full error chain (anyhow chained errors).
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("depth_estimation"), "error must mention the bad value: {msg}");
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn unknown_operator_returns_error() {
        let yaml = r"
requirement_uid: REQ-BAD-OP
use_case: classification
constraints:
  - metric: return_code
    operator: between
    value: 0
";
        let result = parse_yaml(yaml);
        assert!(result.is_err(), "unknown operator must be an error");
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn unknown_metric_returns_error() {
        let yaml = r"
requirement_uid: REQ-BAD-METRIC
use_case: classification
constraints:
  - metric: no.such.metric
    operator: eq
    value: 0
";
        let result = parse_yaml(yaml);
        assert!(result.is_err(), "unknown metric must be an error");
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("no.such.metric"), "error must mention the bad metric: {msg}");
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn bare_number_value_is_accepted() {
        let yaml = r"
requirement_uid: REQ-BARE
use_case: classification
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let spec = parse_yaml(yaml).expect("bare number should parse");
        assert!((spec.constraints[0].expected() - 0.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-PERF-004
    #[test]
    fn timeout_falls_back_to_default() {
        let yaml = r"
requirement_uid: REQ-TIMEOUT-DEFAULT
use_case: classification
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let raw: RawTestSpec = serde_yaml::from_str(yaml).unwrap();
        let spec = convert_spec(raw, 600).unwrap();
        assert_eq!(spec.target.timeout, Duration::from_mins(10));
    }

    /// Verifies: SRS-PERF-004
    #[test]
    fn per_spec_timeout_overrides_default() {
        let yaml = r"
requirement_uid: REQ-TIMEOUT-OVERRIDE
use_case: classification
timeout_secs: 30
constraints:
  - metric: return_code
    operator: eq
    value: 0
";
        let spec = parse_yaml(yaml).unwrap();
        assert_eq!(spec.target.timeout, Duration::from_secs(30));
    }

    /// Verifies: SRS-FUNC-019
    #[test]
    fn all_use_case_variants_parse() {
        let cases = [
            ("object_detection", WorkloadType::ObjectDetection),
            ("image_classification", WorkloadType::ImageClassification),
            ("pixel_segmentation", WorkloadType::PixelSegmentation),
            ("classification", WorkloadType::Classification),
            ("regression", WorkloadType::Regression),
            ("time_series_forecasting", WorkloadType::TimeSeriesForecasting),
        ];
        for (s, expected) in &cases {
            let result = parse_use_case(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(result, *expected, "use_case '{s}' mismatch");
        }
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn all_operators_parse() {
        let cases = [
            ("lte", Op::Lte),
            ("gte", Op::Gte),
            ("lt", Op::Lt),
            ("gt", Op::Gt),
            ("eq", Op::Eq),
            ("ne", Op::Ne),
        ];
        for (s, expected) in &cases {
            let result = parse_operator(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert_eq!(result, *expected, "operator '{s}' mismatch");
        }
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn time_unit_conversions() {
        assert!((parse_unit_value("100ms", "time.avg_ms").unwrap() - 100.0).abs() < f64::EPSILON);
        assert!((parse_unit_value("1.5s", "time.avg_ms").unwrap() - 1_500.0).abs() < f64::EPSILON);
        assert!((parse_unit_value("2s", "time.avg_ms").unwrap() - 2_000.0).abs() < f64::EPSILON);
    }

    /// Verifies: SRS-FUNC-004
    #[test]
    fn memory_unit_conversions() {
        assert!(
            (parse_unit_value("512KB", "memory.peak_rss_kb").unwrap() - 512.0).abs() < f64::EPSILON
        );
        assert!(
            (parse_unit_value("512 KB", "memory.peak_rss_kb").unwrap() - 512.0).abs()
                < f64::EPSILON
        );
        assert!(
            (parse_unit_value("2MB", "memory.peak_rss_kb").unwrap() - 2_048.0).abs() < f64::EPSILON
        );
        assert!(
            (parse_unit_value("1GB", "memory.peak_rss_kb").unwrap() - 1_048_576.0).abs()
                < f64::EPSILON
        );
    }

    /// Verifies: SRS-FUNC-003, SRS-MAINT-002
    #[test]
    fn load_specs_from_temp_files() {
        use std::io::Write;

        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r"
requirement_uid: REQ-FILE-001
use_case: image_classification
constraints:
  - metric: return_code
    operator: eq
    value: 0
"
        )
        .unwrap();

        let specs = load_specs(&[f.path().to_path_buf()], 300).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].requirement_uid, "REQ-FILE-001");
    }

    /// Verifies: SRS-FUNC-003
    #[test]
    fn load_specs_bad_file_returns_error() {
        let result = load_specs(&[PathBuf::from("/nonexistent/path/spec.yaml")], 300);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("nonexistent"), "error should mention the bad path: {msg}");
    }
}
