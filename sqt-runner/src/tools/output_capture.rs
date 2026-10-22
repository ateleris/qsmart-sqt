//! Capture the binary-under-test's inference output into a `JobOutput`
//! (F-H1, SRS-FUNC-016/017, SRS-REL-004).
//!
//! The reference runner treats the binary's **stdout** as its inference
//! output. Whatever the binary prints is surfaced to the reporter so it can
//! compute precision/accuracy against a reference and record output
//! consistency — logic that was previously dead because the runner always
//! returned `output: null`.

use std::fmt::Write as _;

use sha1::{Digest, Sha1};
use sqt_types::JobOutput;

/// Build a [`JobOutput`] from the binary's captured stdout.
///
/// Returns `None` when the binary produced no stdout (nothing to report).
/// `output_type` is a caller-supplied semantic label (the workload type).
///
/// `result_consistent` is `true`: the reference runner currently executes the
/// binary once, so "every repetition produced identical output" holds
/// vacuously. Cross-repetition comparison arrives with multi-run support
/// (F-M6).
pub fn build_job_output(output_type: &str, stdout: &[u8]) -> Option<JobOutput> {
    if stdout.iter().all(u8::is_ascii_whitespace) {
        return None;
    }
    let text = String::from_utf8_lossy(stdout);
    Some(JobOutput {
        output_type: output_type.to_string(),
        result_consistent: true,
        outputs: parse_outputs(&text),
        outputs_checksum: Some(checksum(stdout)),
    })
}

/// Interpret the binary's stdout as a list of per-inference outputs.
///
/// - A JSON array becomes the outputs vector as-is.
/// - Any other single JSON value becomes a one-element vector.
/// - Non-JSON text becomes a one-element vector holding the trimmed text as a
///   JSON string, so the reporter always has something to compare/record.
///
/// Returns `None` only for effectively empty input.
fn parse_outputs(text: &str) -> Option<Vec<serde_json::Value>> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    match serde_json::from_str::<serde_json::Value>(trimmed) {
        Ok(serde_json::Value::Array(items)) => Some(items),
        Ok(value) => Some(vec![value]),
        Err(_) => Some(vec![serde_json::Value::String(trimmed.to_string())]),
    }
}

/// Lower-case hex SHA-1 of the raw output bytes. Used as evidence and, once
/// multi-run support lands, to compare outputs across repetitions.
fn checksum(bytes: &[u8]) -> String {
    let mut hasher = Sha1::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verifies: SRS-INST-001
    #[test]
    fn json_array_stdout_becomes_outputs_vector() {
        let out = build_job_output("classification", br#"[{"label":"cat"},{"label":"dog"}]"#)
            .expect("output present");
        let outputs = out.outputs.expect("outputs present");
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0]["label"], "cat");
        assert!(out.result_consistent);
        assert_eq!(out.output_type, "classification");
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn single_json_object_becomes_one_element_vector() {
        let out = build_job_output("regression", br#"{"prediction":0.87}"#).unwrap();
        let outputs = out.outputs.unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0]["prediction"], 0.87);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn non_json_stdout_is_wrapped_as_string() {
        let out = build_job_output("raw", b"hello world\n").unwrap();
        let outputs = out.outputs.unwrap();
        assert_eq!(outputs, vec![serde_json::Value::String("hello world".to_string())]);
    }

    /// Verifies: SRS-INST-001
    #[test]
    fn empty_or_whitespace_stdout_yields_no_output() {
        assert!(build_job_output("raw", b"").is_none());
        assert!(build_job_output("raw", b"   \n\t ").is_none());
    }

    /// Verifies: SRS-INST-001, SRS-REL-004
    #[test]
    fn checksum_is_stable_hex_sha1() {
        // Known SHA-1 of "abc".
        assert_eq!(checksum(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
    }

    /// Verifies: SRS-INST-001, SRS-REL-004
    #[test]
    fn identical_bytes_share_a_checksum() {
        let a = build_job_output("x", b"same output").unwrap();
        let b = build_job_output("x", b"same output").unwrap();
        assert_eq!(a.outputs_checksum, b.outputs_checksum);
    }
}
