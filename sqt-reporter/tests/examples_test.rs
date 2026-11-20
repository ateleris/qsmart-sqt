// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! The example configuration and specifications under `examples/` load with
//! the current parser, so the files operators start from stay valid.

use std::path::Path;

use sqt_reporter::{config, specs};

fn examples() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples"))
}

/// Verifies: SRS-FUNC-001, SRS-FUNC-003
#[test]
fn example_configuration_and_every_example_spec_load() {
    let cfg = config::load_config(&examples().join("sqt-reporter.example.yaml"))
        .expect("examples/sqt-reporter.example.yaml loads");
    let loaded = specs::loader::load_specs(&cfg.test_specs.files, cfg.runner.job_timeout_secs)
        .expect("every spec listed in the example configuration loads");
    assert_eq!(loaded.len(), cfg.test_specs.files.len(), "one specification per file");

    // Spec files in examples/specs that the configuration does not list load too.
    for entry in std::fs::read_dir(examples().join("specs")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "yaml") {
            specs::loader::load_specs(std::slice::from_ref(&path), 60)
                .unwrap_or_else(|e| panic!("{} does not load: {e:#}", path.display()));
        }
    }
}
