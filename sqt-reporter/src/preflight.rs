//! Pre-flight check for `sqt run --preflight` (SRS-OPS-007).
//!
//! Validates config, checks Runner connectivity, and verifies spec files
//! without submitting any jobs.

use anyhow::anyhow;

use crate::api::ApiClient;
use crate::config::SqtConfig;
use crate::specs::loader::load_specs;

/// Run all pre-flight checks.
///
/// Checks performed (in order):
/// 1. `GET /api/v1/version` — Runner connectivity and auth
/// 2. `GET /api/v1/docs`   — Document discovery (warns if empty)
/// 3. Load spec files from `config.test_specs.files`
///
/// Each step is attempted independently; failures are reported inline.
/// Returns `Ok(())` only when every step passes, `Err(...)` if any step failed.
///
/// # Errors
///
/// Returns an error summarising which steps failed.
pub async fn preflight(config: &SqtConfig, client: &ApiClient) -> anyhow::Result<()> {
    println!("Pre-flight check starting...");

    let mut failed = false;

    // ------------------------------------------------------------------
    // Step 1: GET /api/v1/version
    // ------------------------------------------------------------------
    match client.get_version().await {
        Ok(v) => {
            println!(
                "  [OK]   Runner version: {} (api_version: {}, target_id: {})",
                v.runner_version, v.api_version, v.target_id
            );
        }
        Err(e) => {
            println!("  [FAIL] Runner version check failed: {e:#}");
            failed = true;
        }
    }

    // ------------------------------------------------------------------
    // Step 2: GET /api/v1/docs
    // ------------------------------------------------------------------
    match client.get_docs().await {
        Ok(docs) => {
            let count = docs.documents.len();
            if count == 0 {
                println!(
                    "  [WARN] Docs endpoint returned 0 documents — no StrictDoc sources found"
                );
            } else {
                println!("  [OK]   Docs endpoint returned {count} document(s)");
            }
        }
        Err(e) => {
            println!("  [FAIL] Docs endpoint check failed: {e:#}");
            failed = true;
        }
    }

    // ------------------------------------------------------------------
    // Step 3: Load spec files
    // ------------------------------------------------------------------
    let default_timeout = config.runner.job_timeout_secs;
    match load_specs(&config.test_specs.files, default_timeout) {
        Ok(specs) => {
            println!("  [OK]   Loaded {} spec(s) from test_specs.files", specs.len());
        }
        Err(e) => {
            println!("  [FAIL] Spec loading failed: {e:#}");
            failed = true;
        }
    }

    // ------------------------------------------------------------------
    // Summary
    // ------------------------------------------------------------------
    if failed {
        println!("Pre-flight FAILED — see above");
        Err(anyhow!("one or more pre-flight checks failed"))
    } else {
        println!("Pre-flight OK");
        Ok(())
    }
}
