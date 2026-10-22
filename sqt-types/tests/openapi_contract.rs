// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! Contract drift guard (F-H2, SRS-IF-001).
//!
//! Fails when `job-api.openapi.yaml` and the Rust DTOs disagree on field names.
//! The historic break this guards: the contract used `model` where the code
//! (and `ARCHITECTURE.md`) use `use_case`, so a conformant client generated
//! from the `OpenAPI` file could not talk to the runner.

use std::collections::BTreeSet;

use serde_yaml::Value;
use sqt_types::JobMetrics;

fn openapi() -> Value {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../job-api.openapi.yaml");
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_yaml::from_str(&text).expect("job-api.openapi.yaml is not valid YAML")
}

/// Property names declared for `components.schemas.<schema>`.
fn schema_properties(doc: &Value, schema: &str) -> BTreeSet<String> {
    doc["components"]["schemas"][schema]["properties"]
        .as_mapping()
        .unwrap_or_else(|| panic!("schema `{schema}` has no `properties` map"))
        .keys()
        .map(|k| k.as_str().expect("property key is a string").to_string())
        .collect()
}

/// Verifies: SRS-IF-001
#[test]
fn job_request_uses_use_case_not_model() {
    let props = schema_properties(&openapi(), "JobRequest");
    assert!(props.contains("use_case"), "JobRequest must expose `use_case`");
    assert!(!props.contains("model"), "JobRequest must not carry the stale `model` field");
    assert!(
        !props.contains("batch_size"),
        "`batch_size` is not implemented by the DTO and must not be in the contract"
    );
}

/// Verifies: SRS-IF-001
#[test]
fn job_status_response_uses_use_case_not_model() {
    let props = schema_properties(&openapi(), "JobStatusResponse");
    assert!(props.contains("use_case"), "JobStatusResponse must expose `use_case`");
    assert!(!props.contains("model"), "JobStatusResponse must not carry the stale `model` field");
}

/// Verifies: SRS-IF-001
#[test]
fn job_metrics_properties_match_dto_exactly() {
    // JobMetrics has no `skip_serializing_if`, so serializing a default value
    // yields every wire name the DTO knows — the authoritative key set.
    let dto_keys: BTreeSet<String> = serde_json::to_value(JobMetrics::default())
        .expect("serialize JobMetrics")
        .as_object()
        .expect("JobMetrics serializes to an object")
        .keys()
        .cloned()
        .collect();

    let contract_keys = schema_properties(&openapi(), "JobMetrics");

    assert_eq!(
        contract_keys,
        dto_keys,
        "JobMetrics drift between OpenAPI and the DTO.\n  only in contract: {:?}\n  only in DTO: {:?}",
        contract_keys.difference(&dto_keys).collect::<Vec<_>>(),
        dto_keys.difference(&contract_keys).collect::<Vec<_>>(),
    );
}
