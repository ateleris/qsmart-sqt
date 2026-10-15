use std::{
    future::Future,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use reqwest::header::HeaderValue;
use sqt_types::{
    DocsResponse, ErrorResponse, JobAccepted, JobRequest, JobStatus, JobStatusResponse,
    VersionResponse,
};

// ---------------------------------------------------------------------------
// Credential redaction newtype
// ---------------------------------------------------------------------------

/// Wraps a secret string so it never appears in logs or debug output.
#[derive(Clone)]
struct Redacted(String);

impl std::fmt::Debug for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl std::fmt::Display for Redacted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

// ---------------------------------------------------------------------------
// ApiClient
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    /// Bearer token sent per-request via `Authorization: Bearer …`.
    /// Stored as a `Redacted` newtype so `format!("{:?}", client)` never leaks it.
    token: Option<Redacted>,
    retry_max: u32,
    retry_backoff_initial_secs: u64,
    /// Initial delay between job-status polls; doubles each poll up to
    /// `poll_interval_max_secs` (SRS-PERF-003).
    poll_interval_initial_secs: u64,
    poll_interval_max_secs: u64,
}

impl ApiClient {
    /// Create a new `ApiClient`.
    ///
    /// If `token` is `Some`, it is sent as a `Bearer` token in every request.
    /// `retry_max` and `retry_backoff_initial_secs` configure the transient-error
    /// retry loop (exponential back-off).
    ///
    /// # Errors
    ///
    /// Returns an error if the token contains invalid HTTP header characters.
    pub fn new(
        base_url: &str,
        token: Option<&str>,
        retry_max: u32,
        retry_backoff_initial_secs: u64,
        poll_interval_initial_secs: u64,
        poll_interval_max_secs: u64,
    ) -> Result<Self> {
        // Validate the token up-front so `new` fails fast on bad characters.
        if let Some(tok) = token {
            HeaderValue::from_str(&format!("Bearer {tok}"))
                .context("API token contains invalid header characters")?;
        }
        let client = reqwest::Client::builder().build().context("Failed to build HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.map(|t| Redacted(t.to_owned())),
            retry_max,
            retry_backoff_initial_secs,
            poll_interval_initial_secs,
            poll_interval_max_secs,
        })
    }

    /// Attach the `Authorization` header to a request builder if a token is configured.
    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(Redacted(ref tok)) = self.token { rb.bearer_auth(tok) } else { rb }
    }

    // -----------------------------------------------------------------------
    // Retry helper
    // -----------------------------------------------------------------------

    /// Retry `op` up to `retry_max` additional times on transient errors.
    ///
    /// Transient errors: connection failure, timeout, or HTTP 5xx.
    /// Non-transient errors (e.g. 401, 403, 422) are returned immediately.
    async fn retry<F, Fut, T>(&self, op: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = std::result::Result<T, reqwest::Error>>,
    {
        let mut attempt = 0u32;
        loop {
            match op().await {
                Ok(v) => return Ok(v),
                Err(e) if attempt == self.retry_max => return Err(e.into()),
                Err(e)
                    if e.is_connect()
                        || e.is_timeout()
                        || e.status().is_some_and(|s| s.is_server_error()) =>
                {
                    let backoff =
                        Duration::from_secs(self.retry_backoff_initial_secs * 2u64.pow(attempt));
                    tracing::warn!(
                        attempt,
                        backoff_secs = backoff.as_secs(),
                        "transient error; retrying"
                    );
                    tokio::time::sleep(backoff).await;
                    attempt += 1;
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    // -----------------------------------------------------------------------
    // Version endpoint  (GET /api/v1/version)
    // -----------------------------------------------------------------------

    /// Fetch the runner version descriptor.
    ///
    /// Used by the orchestrator at session start to verify API compatibility.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails after retries or the response
    /// cannot be deserialised.
    pub async fn get_version(&self) -> Result<VersionResponse> {
        let url = format!("{}/api/v1/version", self.base_url);
        let client = &self.client;
        let resp = self
            .retry(|| async { self.auth(client.get(&url)).send().await?.error_for_status() })
            .await
            .with_context(|| format!("GET {url} failed"))?;
        let resp: VersionResponse =
            resp.json::<VersionResponse>().await.context("Failed to parse VersionResponse")?;
        validate_version_response(&resp)?;
        Ok(resp)
    }

    // -----------------------------------------------------------------------
    // Docs endpoint  (GET /api/v1/docs)
    // -----------------------------------------------------------------------

    /// Fetch the `StrictDoc` documents from the runner.
    ///
    /// Emits a warning when `documents` is empty (SRS-FUNC-018 empty-discovery
    /// diagnostic).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails after retries or the response
    /// cannot be deserialised.
    pub async fn get_docs(&self) -> Result<DocsResponse> {
        let url = format!("{}/api/v1/docs", self.base_url);
        let client = &self.client;
        let resp = self
            .retry(|| async { self.auth(client.get(&url)).send().await?.error_for_status() })
            .await
            .with_context(|| format!("GET {url} failed"))?;
        let docs: DocsResponse = resp.json().await.context("Failed to parse DocsResponse")?;
        if docs.documents.is_empty() {
            tracing::warn!("GET /api/v1/docs returned empty documents array");
        }
        Ok(docs)
    }

    // -----------------------------------------------------------------------
    // Job endpoint  (POST /api/v1/jobs  +  GET /api/v1/jobs/{job_id})
    // -----------------------------------------------------------------------

    /// Submit a qualification job.
    ///
    /// Returns the `job_id` on success (202 Accepted).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails after retries or the server
    /// responds with a non-success status.
    pub async fn submit_job(&self, request: &JobRequest) -> Result<String> {
        let url = format!("{}/api/v1/jobs", self.base_url);
        let client = &self.client;

        // Retry on transient errors; preserve the response body on terminal failures
        // so we can surface the structured ErrorResponse.
        let response = self
            .retry(|| {
                let req = self.auth(client.post(&url).json(request));
                async move {
                    let r = req.send().await?;
                    // Convert 5xx into an error so the retry helper can detect server errors,
                    // but only do so — non-5xx are returned as-is for body inspection below.
                    if r.status().is_server_error() { r.error_for_status() } else { Ok(r) }
                }
            })
            .await
            .with_context(|| format!("POST {url} failed"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("API error {status}: {} - {}", err.error.code, err.error.message);
            }
            bail!("API returned {status}: {body}");
        }

        let accepted: JobAccepted =
            response.json().await.context("Failed to parse JobAccepted response")?;
        Ok(accepted.job_id)
    }

    /// Retrieve the current status of a job.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails after retries or the response
    /// cannot be deserialised.
    pub async fn get_job_status(&self, job_id: &str) -> Result<JobStatusResponse> {
        let url = format!("{}/api/v1/jobs/{job_id}", self.base_url);
        let client = &self.client;
        let response = self
            .retry(|| async {
                let r = self.auth(client.get(&url)).send().await?;
                if r.status().is_server_error() { r.error_for_status() } else { Ok(r) }
            })
            .await
            .with_context(|| format!("GET {url} failed"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("API error {status}: {} - {}", err.error.code, err.error.message);
            }
            bail!("API returned {status}: {body}");
        }

        let resp: JobStatusResponse =
            response.json().await.context("Failed to parse JobStatusResponse")?;
        validate_job_status(&resp)?;
        Ok(resp)
    }

    /// Poll until the job reaches `complete`, or fail fast on any terminal error.
    ///
    /// Uses exponential backoff between polls, configured by
    /// `poll_interval_initial_secs` (×2 per poll) up to `poll_interval_max_secs`
    /// (SRS-PERF-003). Bails immediately when the server reports `failed`,
    /// embedding the server-side error field. Bails with a timeout error after
    /// `timeout`.
    ///
    /// # Errors
    ///
    /// Returns an error if a poll request fails, the job transitions to `failed`,
    /// or `timeout` is exceeded before the job completes.
    pub async fn wait_for_job(&self, job_id: &str, timeout: Duration) -> Result<JobStatusResponse> {
        // Clamp to a sane floor so a misconfigured 0 can't busy-loop.
        let max_delay = Duration::from_secs(self.poll_interval_max_secs.max(1));
        let deadline = Instant::now() + timeout;
        let mut delay = Duration::from_secs(self.poll_interval_initial_secs.max(1)).min(max_delay);

        loop {
            let resp = self.get_job_status(job_id).await?;
            tracing::debug!(job_id, status = ?resp.status, "polled job status");

            if resp.status == JobStatus::Failed {
                let msg = resp.error.as_deref().unwrap_or("(no detail provided)");
                bail!("job {job_id} failed on server: {msg}");
            }
            if resp.status == JobStatus::Complete {
                return Ok(resp);
            }

            if Instant::now() >= deadline {
                bail!(
                    "job {job_id} timed out after {timeout:.1?} — last status: {:?}",
                    resp.status
                );
            }

            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(max_delay);
        }
    }
}

// ---------------------------------------------------------------------------
// Response schema validators  (SRS-IF-007)
// ---------------------------------------------------------------------------

/// Validate required fields of a `JobStatusResponse`.
///
/// A missing or empty `job_id` indicates a non-conformant response; the caller
/// records the affected job as infrastructure-failed.
fn validate_job_status(resp: &sqt_types::JobStatusResponse) -> anyhow::Result<()> {
    if resp.job_id.is_empty() {
        anyhow::bail!("schema: job_id field is empty in JobStatusResponse");
    }
    Ok(())
}

/// Validate required fields of a `VersionResponse`.
///
/// Both `api_version` and `runner_version` must be non-empty for the response
/// to be considered conformant.
fn validate_version_response(resp: &sqt_types::VersionResponse) -> anyhow::Result<()> {
    if resp.api_version.is_empty() {
        anyhow::bail!("schema: api_version field is empty in VersionResponse");
    }
    if resp.runner_version.is_empty() {
        anyhow::bail!("schema: runner_version field is empty in VersionResponse");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_client(retry_max: u32) -> ApiClient {
        ApiClient::new("http://localhost:9999", Some("secret-token"), retry_max, 0, 1, 30)
            .expect("client should build")
    }

    #[test]
    fn token_not_in_debug_output() {
        let client = make_client(3);
        let debug = format!("{client:?}");
        assert!(
            !debug.contains("secret-token"),
            "raw token must not appear in Debug output: {debug}"
        );
        assert!(debug.contains("[REDACTED]"), "Debug output should contain [REDACTED]: {debug}");
    }

    #[test]
    fn token_not_in_display_of_redacted() {
        let r = Redacted("my-secret".to_owned());
        assert_eq!(format!("{r}"), "[REDACTED]");
        assert_eq!(format!("{r:?}"), "[REDACTED]");
    }

    #[test]
    fn client_builds_without_token() {
        let client = ApiClient::new("http://localhost:9999", None, 3, 1, 1, 30)
            .expect("client without token should build");
        assert!(client.token.is_none());
    }

    #[test]
    fn client_builds_with_retry_and_poll_params() {
        let client = ApiClient::new("http://localhost:9999", Some("tok"), 5, 2, 3, 45)
            .expect("client should build");
        assert_eq!(client.retry_max, 5);
        assert_eq!(client.retry_backoff_initial_secs, 2);
        // SRS-PERF-003: poll cadence must come from config, not be hardcoded.
        assert_eq!(client.poll_interval_initial_secs, 3);
        assert_eq!(client.poll_interval_max_secs, 45);
    }

    // -----------------------------------------------------------------------
    // SRS-IF-007: response schema validation
    // -----------------------------------------------------------------------

    fn make_job_status_resp(job_id: &str) -> sqt_types::JobStatusResponse {
        sqt_types::JobStatusResponse {
            job_id: job_id.to_owned(),
            use_case: Default::default(),
            mode: Default::default(),
            repetitions: 1,
            status: sqt_types::JobStatus::Accepted,
            queue_position: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            started_at: None,
            completed_at: None,
            error: None,
            tags: None,
            hardware: None,
            metrics: Default::default(),
            output: None,
        }
    }

    #[test]
    fn validate_job_status_rejects_empty_job_id() {
        let resp = make_job_status_resp("");
        let err = validate_job_status(&resp).unwrap_err();
        assert!(err.to_string().contains("job_id"), "error should mention job_id: {err}");
    }

    #[test]
    fn validate_job_status_accepts_valid_response() {
        let resp = make_job_status_resp("job-abc");
        assert!(validate_job_status(&resp).is_ok());
    }

    fn make_version_resp(api_version: &str, runner_version: &str) -> sqt_types::VersionResponse {
        sqt_types::VersionResponse {
            api_version: api_version.to_owned(),
            runner_version: runner_version.to_owned(),
            target_id: "test-target".to_owned(),
            os: "linux".to_owned(),
            toolchain_versions: std::collections::HashMap::new(),
        }
    }

    #[test]
    fn validate_version_response_rejects_empty_api_version() {
        let resp = make_version_resp("", "1.0.0");
        let err = validate_version_response(&resp).unwrap_err();
        assert!(err.to_string().contains("api_version"), "error should mention api_version: {err}");
    }

    #[test]
    fn validate_version_response_rejects_empty_runner_version() {
        let resp = make_version_resp("v1", "");
        let err = validate_version_response(&resp).unwrap_err();
        assert!(
            err.to_string().contains("runner_version"),
            "error should mention runner_version: {err}"
        );
    }

    #[test]
    fn validate_version_response_accepts_valid_response() {
        let resp = make_version_resp("v1", "1.2.3");
        assert!(validate_version_response(&resp).is_ok());
    }
}
