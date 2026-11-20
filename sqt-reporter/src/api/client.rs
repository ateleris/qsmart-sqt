// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

//! HTTP client for the Runner API: the four contract operations the
//! Orchestrator performs (SRS-IF-001), with bearer authentication
//! (SRS-IF-006), bounded retry (SRS-FUNC-020), polling with back-off
//! (SRS-IF-003, SRS-PERF-003), response validation (SRS-IF-007) and a record
//! of every response body for the evidence (SRS-FUNC-008).

use std::{
    sync::Mutex,
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
// Recorded responses
// ---------------------------------------------------------------------------

/// A Runner response body exactly as received, kept as run evidence
/// (SRS-FUNC-008). Request headers, and so the bearer token, are never recorded.
#[derive(Debug, Clone)]
pub struct RecordedResponse {
    /// Contract operation: `get_version`, `get_docs`, `submit_job` or `get_job_status`.
    pub operation: &'static str,
    /// Request URL.
    pub url: String,
    /// HTTP status code.
    pub status: u16,
    /// RFC 3339 UTC time the body was received.
    pub received_at: String,
    /// The job the response belongs to, when known.
    pub job_id: Option<String>,
    /// Response body as received.
    pub body: Vec<u8>,
}

// ---------------------------------------------------------------------------
// ApiClient
// ---------------------------------------------------------------------------

/// Timing and retry settings of an [`ApiClient`] (the `runner:` block of the
/// configuration).
#[derive(Debug, Clone, Copy)]
pub struct ClientOptions {
    /// Additional attempts after a transient failure (SRS-FUNC-020).
    pub retry_max: u32,
    /// Back-off before the first retry; doubles for each further retry.
    pub retry_backoff_initial_secs: u64,
    /// Initial delay between job-status polls; doubles each poll up to
    /// `poll_interval_max_secs` (SRS-PERF-003).
    pub poll_interval_initial_secs: u64,
    /// Upper bound for the delay between job-status polls.
    pub poll_interval_max_secs: u64,
    /// Upper bound on one HTTP request, body included.
    pub request_timeout_secs: u64,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            retry_max: 3,
            retry_backoff_initial_secs: 1,
            poll_interval_initial_secs: 1,
            poll_interval_max_secs: 30,
            request_timeout_secs: 30,
        }
    }
}

impl From<&crate::config::RunnerConfig> for ClientOptions {
    fn from(r: &crate::config::RunnerConfig) -> Self {
        Self {
            retry_max: r.retry_max,
            retry_backoff_initial_secs: r.retry_backoff_initial_secs,
            poll_interval_initial_secs: r.poll_interval_initial_secs,
            poll_interval_max_secs: r.poll_interval_max_secs,
            request_timeout_secs: r.request_timeout_secs,
        }
    }
}

/// Connection set-up is bounded separately, so an unreachable Runner fails
/// fast even when requests may take long.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Check that `url` may carry the bearer token (SRS-IF-010): over HTTPS, or
/// over plain HTTP only to the local host (`localhost`, `127.0.0.0/8`, `::1`,
/// and the IPv4-mapped `::ffff:127.0.0.0/104`).
///
/// The server certificate of an HTTPS Runner is always verified; the client
/// offers no way to turn that off. A plain-HTTP URL also never goes through a
/// proxy from the environment ([`env_proxy_allowed`]), so the token stays on
/// the local machine.
///
/// # Errors
///
/// Returns an error if `url` is not a valid URL, or if it is a plain-HTTP URL
/// to any other host. The message shows the URL without user name, password,
/// query or fragment, which may carry credentials (SRS-OPS-005).
pub fn check_token_transport(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)
        .context("the Runner URL is not a valid URL (not shown, it may contain credentials)")?;
    if parsed.scheme() == "http" && !is_local_host(&parsed) {
        bail!(
            "the Runner URL {} uses plain http:// to a host other than the local machine; \
             the bearer token is sent only over https:// or to localhost (SRS-IF-010)",
            display_url(&parsed)
        );
    }
    Ok(())
}

/// Whether the client may use a proxy configured in the environment
/// (`HTTP_PROXY`, `ALL_PROXY`, ...) for `url`. Not for plain HTTP: such a URL
/// is only allowed to the local host, and a proxy would carry the bearer token
/// unencrypted to another machine (SRS-IF-010). An HTTPS request through a
/// proxy stays encrypted end to end, so proxies remain allowed there.
///
/// This decision is a function of its own because the proxy itself is read
/// from environment variables, which a test cannot set safely.
pub fn env_proxy_allowed(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| u.scheme() != "http")
}

/// Whether the URL's host is the local machine: `localhost` (also with a
/// trailing dot) or a loopback address, IPv4-mapped IPv6 included.
fn is_local_host(url: &reqwest::Url) -> bool {
    // host_str() gives an IPv6 address in brackets, e.g. "[::1]".
    let host = url.host_str().unwrap_or_default().trim_start_matches('[').trim_end_matches(']');
    if host.strip_suffix('.').unwrap_or(host).eq_ignore_ascii_case("localhost") {
        return true;
    }
    match host.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback(),
        Ok(std::net::IpAddr::V6(ip)) => {
            ip.is_loopback() || ip.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
        Err(_) => false,
    }
}

/// `url` for a message: without user name, password, query and fragment.
fn display_url(url: &reqwest::Url) -> String {
    let mut shown = url.clone();
    // Both fail only for URLs that cannot have credentials; nothing to strip then.
    let _ = shown.set_username("");
    let _ = shown.set_password(None);
    shown.set_query(None);
    shown.set_fragment(None);
    shown.to_string()
}

/// Whether a request may be sent again after a failure (SRS-FUNC-020).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Resend {
    /// Reads (`GET`): any transient failure may be retried.
    Always,
    /// Job submission (`POST`). The contract has no idempotency key, so the
    /// request is sent again only when the Runner cannot have created a job:
    /// the connection was never established, or the Runner answered that it
    /// did not accept the request (503, 429). A timeout or reset after the
    /// request went out is not retried; it could duplicate the job.
    OnlyIfNotAccepted,
}

impl Resend {
    fn retries_status(self, status: reqwest::StatusCode) -> bool {
        use reqwest::StatusCode as S;
        match self {
            Self::Always => matches!(
                status,
                S::BAD_GATEWAY | S::SERVICE_UNAVAILABLE | S::GATEWAY_TIMEOUT | S::TOO_MANY_REQUESTS
            ),
            Self::OnlyIfNotAccepted => {
                matches!(status, S::SERVICE_UNAVAILABLE | S::TOO_MANY_REQUESTS)
            }
        }
    }

    /// An error before a response arrived: connect failure, timeout, or the
    /// connection dropped while the request was sent (network reset).
    fn retries_send_error(self, e: &reqwest::Error) -> bool {
        match self {
            Self::Always => e.is_connect() || e.is_timeout() || e.is_request(),
            Self::OnlyIfNotAccepted => e.is_connect(),
        }
    }

    /// The response body could not be read to the end (reset or timeout
    /// mid-response). reqwest decodes nothing here, so this is always a
    /// transport failure; the request reached the Runner.
    fn retries_body_error(self) -> bool {
        self == Self::Always
    }
}

/// Client for one Runner endpoint. Holds the bearer token, the retry and
/// polling settings, and the responses received so far.
#[derive(Debug)]
pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
    /// Bearer token sent per-request via `Authorization: Bearer …`.
    /// Stored as a `Redacted` newtype so `format!("{:?}", client)` never leaks it.
    token: Option<Redacted>,
    options: ClientOptions,
    /// Every response body received, in arrival order (see [`Self::take_recorded`]).
    recorded: Mutex<Vec<RecordedResponse>>,
}

impl ApiClient {
    /// Create a new `ApiClient`.
    ///
    /// If `token` is `Some`, it is sent as a `Bearer` token in every request.
    ///
    /// # Errors
    ///
    /// Returns an error if the token contains invalid HTTP header characters,
    /// or if a token is given for a URL that may not carry it
    /// ([`check_token_transport`], SRS-IF-010).
    pub fn new(base_url: &str, token: Option<&str>, options: ClientOptions) -> Result<Self> {
        // Validate the token up-front so `new` fails fast on bad characters.
        if let Some(tok) = token {
            HeaderValue::from_str(&format!("Bearer {tok}"))
                .context("API token contains invalid header characters")?;
            check_token_transport(base_url)?;
        }
        // The SQT contacts only the configured Runner endpoint (SRS-RES-002):
        // a redirect to another location is not followed but fails the request.
        let mut builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(options.request_timeout_secs.max(1)));
        if !env_proxy_allowed(base_url) {
            // Plain HTTP goes straight to the local Runner (SRS-IF-010).
            builder = builder.no_proxy();
        }
        let client = builder.build().context("Failed to build HTTP client")?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
            token: token.map(|t| Redacted(t.to_owned())),
            options,
            recorded: Mutex::new(Vec::new()),
        })
    }

    /// Take every response recorded so far, oldest first, leaving the record empty.
    pub fn take_recorded(&self) -> Vec<RecordedResponse> {
        std::mem::take(
            &mut *self.recorded.lock().unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }

    fn record(
        &self,
        operation: &'static str,
        url: &str,
        status: reqwest::StatusCode,
        job_id: Option<&str>,
        body: &[u8],
    ) {
        let entry = RecordedResponse {
            operation,
            url: url.to_owned(),
            status: status.as_u16(),
            received_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            job_id: job_id.map(str::to_owned),
            body: body.to_vec(),
        };
        self.recorded.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(entry);
    }

    /// `text` with every occurrence of the bearer token replaced, for logging a
    /// payload (SRS-OPS-005): a Runner may echo the token in a response.
    fn redact<'a>(&self, text: &'a str) -> std::borrow::Cow<'a, str> {
        match self.token {
            Some(Redacted(ref tok)) if !tok.is_empty() && text.contains(tok.as_str()) => {
                std::borrow::Cow::Owned(text.replace(tok.as_str(), "[REDACTED]"))
            }
            _ => std::borrow::Cow::Borrowed(text),
        }
    }

    /// Attach the `Authorization` header to a request builder if a token is configured.
    fn auth(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        if let Some(Redacted(ref tok)) = self.token { rb.bearer_auth(tok) } else { rb }
    }

    // -----------------------------------------------------------------------
    // Sending with bounded retry (SRS-FUNC-020)
    // -----------------------------------------------------------------------

    /// Send the request built by `build` and read its whole body, retrying up
    /// to `retry_max` more times on a transient failure, with back-off
    /// doubling from `retry_backoff_initial_secs`. What counts as transient
    /// depends on `resend`.
    ///
    /// Returns the status and body of the last response; the caller handles a
    /// non-success status. A response that triggers a retry is recorded as
    /// evidence here, the returned one by the caller.
    async fn send(
        &self,
        operation: &'static str,
        url: &str,
        job_id: Option<&str>,
        resend: Resend,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<(reqwest::StatusCode, Vec<u8>)> {
        let mut attempt = 0u32;
        loop {
            let can_retry = attempt < self.options.retry_max;
            tracing::trace!(operation, url, attempt, "Runner request");
            let outcome = match self.auth(build()).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_redirection() {
                        // Not followed (SRS-RES-002); say where it pointed.
                        let location = resp
                            .headers()
                            .get(reqwest::header::LOCATION)
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("(no Location header)")
                            .to_owned();
                        bail!(
                            "the Runner answered {status} with a redirect to {location}; the SQT \
                             contacts only the configured Runner URL and does not follow \
                             redirects, so set runner.url to the final address"
                        );
                    }
                    match resp.bytes().await {
                        Ok(body) => {
                            // The most verbose level logs every response
                            // payload (SRS-OPS-002), without the token.
                            tracing::trace!(
                                operation,
                                url,
                                status = status.as_u16(),
                                payload = %self.redact(&String::from_utf8_lossy(&body)),
                                "Runner response"
                            );
                            Ok((status, body.to_vec()))
                        }
                        Err(e) => Err((e, resend.retries_body_error())),
                    }
                }
                Err(e) => {
                    let transient = resend.retries_send_error(&e);
                    Err((e, transient))
                }
            };
            let reason = match outcome {
                Ok((status, body)) if can_retry && resend.retries_status(status) => {
                    self.record(operation, url, status, job_id, &body);
                    format!("HTTP {status}")
                }
                Ok(response) => return Ok(response),
                Err((e, true)) if can_retry => format!("{e:#}"),
                Err((e, _)) => {
                    let e = anyhow::Error::from(e);
                    return Err(if attempt > 0 {
                        e.context(format!("gave up after {} attempts", attempt + 1))
                    } else {
                        e
                    });
                }
            };
            let backoff = Duration::from_secs(
                self.options
                    .retry_backoff_initial_secs
                    .saturating_mul(2u64.saturating_pow(attempt)),
            );
            attempt += 1;
            tracing::warn!(
                operation,
                attempt,
                retry_max = self.options.retry_max,
                backoff_secs = backoff.as_secs(),
                "transient Runner error ({reason}); retrying"
            );
            tokio::time::sleep(backoff).await;
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
        let (status, body) = self
            .send("get_version", &url, None, Resend::Always, || self.client.get(&url))
            .await
            .with_context(|| format!("GET {url} failed"))?;
        self.record("get_version", &url, status, None, &body);
        if !status.is_success() {
            bail_with_error_body(status, &body).with_context(|| format!("GET {url} failed"))?;
        }
        let resp: VersionResponse =
            serde_json::from_slice(&body).context("Failed to parse VersionResponse")?;
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
        let (status, body) = self
            .send("get_docs", &url, None, Resend::Always, || self.client.get(&url))
            .await
            .with_context(|| format!("GET {url} failed"))?;
        self.record("get_docs", &url, status, None, &body);
        if !status.is_success() {
            bail_with_error_body(status, &body).with_context(|| format!("GET {url} failed"))?;
        }
        let docs: DocsResponse =
            serde_json::from_slice(&body).context("Failed to parse DocsResponse")?;
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
    /// Returns the `job_id` on success (202 Accepted). The submission is sent
    /// again only when the Runner cannot have accepted it (see `Resend`).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails after retries or the server
    /// responds with a non-success status.
    pub async fn submit_job(&self, request: &JobRequest) -> Result<String> {
        let url = format!("{}/api/v1/jobs", self.base_url);
        // The only operation with a request payload; logged at the most
        // verbose level (SRS-OPS-002).
        if tracing::enabled!(tracing::Level::TRACE) {
            match serde_json::to_string(request) {
                Ok(payload) => tracing::trace!(
                    operation = "submit_job",
                    url,
                    payload = %self.redact(&payload),
                    "Runner request payload"
                ),
                Err(e) => tracing::trace!(
                    operation = "submit_job",
                    url,
                    "Runner request payload not loggable: {e}"
                ),
            }
        }
        let (status, body) = self
            .send("submit_job", &url, None, Resend::OnlyIfNotAccepted, || {
                self.client.post(&url).json(request)
            })
            .await
            .with_context(|| format!("POST {url} failed"))?;
        if !status.is_success() {
            self.record("submit_job", &url, status, None, &body);
            bail_with_error_body(status, &body)?;
        }

        let accepted = match serde_json::from_slice::<JobAccepted>(&body) {
            Ok(a) => a,
            Err(e) => {
                self.record("submit_job", &url, status, None, &body);
                return Err(e).context("Failed to parse JobAccepted response");
            }
        };
        self.record("submit_job", &url, status, Some(&accepted.job_id), &body);
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
        let (status, body) = self
            .send("get_job_status", &url, Some(job_id), Resend::Always, || self.client.get(&url))
            .await
            .with_context(|| format!("GET {url} failed"))?;
        self.record("get_job_status", &url, status, Some(job_id), &body);
        if !status.is_success() {
            bail_with_error_body(status, &body)?;
        }

        let resp: JobStatusResponse =
            serde_json::from_slice(&body).context("Failed to parse JobStatusResponse")?;
        validate_job_status(&resp)?;
        Ok(resp)
    }

    /// Poll until the job reaches a terminal state, `complete` or `failed`, and
    /// return that final response (SRS-IF-003).
    ///
    /// A `failed` job is returned, not turned into an error, so the caller can
    /// record it as a Runner-side failure with the Runner's `error` text
    /// (SRS-OPS-006, SRS-REL-003) rather than as a transport error.
    ///
    /// Uses exponential backoff between polls, configured by
    /// `poll_interval_initial_secs` (×2 per poll) up to `poll_interval_max_secs`
    /// (SRS-PERF-003).
    ///
    /// # Errors
    ///
    /// Returns an error if a poll request fails or `timeout` is exceeded before
    /// the job reaches a terminal state.
    pub async fn wait_for_job(&self, job_id: &str, timeout: Duration) -> Result<JobStatusResponse> {
        // Clamp to a sane floor so a misconfigured 0 can't busy-loop.
        let max_delay = Duration::from_secs(self.options.poll_interval_max_secs.max(1));
        let deadline = Instant::now() + timeout;
        let mut delay =
            Duration::from_secs(self.options.poll_interval_initial_secs.max(1)).min(max_delay);

        loop {
            let resp = self.get_job_status(job_id).await?;
            tracing::debug!(job_id, status = ?resp.status, "polled job status");

            if matches!(resp.status, JobStatus::Complete | JobStatus::Failed) {
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

/// Turn a non-success response body into an error, preferring the structured
/// `ErrorResponse` when the body is one.
fn bail_with_error_body(status: reqwest::StatusCode, body: &[u8]) -> Result<()> {
    if let Ok(err) = serde_json::from_slice::<ErrorResponse>(body) {
        bail!("API error {status}: {} - {}", err.error.code, err.error.message);
    }
    bail!("API returned {status}: {}", String::from_utf8_lossy(body));
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
        let options =
            ClientOptions { retry_max, retry_backoff_initial_secs: 0, ..Default::default() };
        ApiClient::new("http://localhost:9999", Some("secret-token"), options)
            .expect("client should build")
    }

    /// Verifies: SRS-OPS-005
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

    /// Verifies: SRS-OPS-005
    #[test]
    fn token_not_in_display_of_redacted() {
        let r = Redacted("my-secret".to_owned());
        assert_eq!(format!("{r}"), "[REDACTED]");
        assert_eq!(format!("{r:?}"), "[REDACTED]");
    }

    /// Verifies: SRS-IF-010
    #[test]
    fn token_travels_only_over_https_or_to_the_local_host() {
        for ok in [
            "https://runner.example",
            "https://10.0.0.5:3000",
            "http://localhost:3000",
            "http://LOCALHOST",
            "http://127.0.0.1:3000",
            "http://127.1.2.3",
            "http://[::1]:3000",
            "http://localhost.:3000",
            "http://[::ffff:127.0.0.1]:3000",
        ] {
            assert!(check_token_transport(ok).is_ok(), "{ok} must be allowed");
        }
        for refused in [
            "http://runner.example",
            "http://10.0.0.5:3000",
            "http://[2001:db8::1]",
            "http://localhost.example",
            "not a url",
        ] {
            assert!(check_token_transport(refused).is_err(), "{refused} must be refused");
        }
    }

    /// Verifies: SRS-IF-010
    #[test]
    fn plain_http_never_uses_an_environment_proxy() {
        assert!(!env_proxy_allowed("http://localhost:3000"));
        assert!(!env_proxy_allowed("http://127.0.0.1:3000"));
        assert!(!env_proxy_allowed("not a url"));
        assert!(env_proxy_allowed("https://runner.example"));
    }

    /// Verifies: SRS-IF-010, SRS-OPS-005
    #[test]
    fn refusal_does_not_show_credentials_from_the_url() {
        let err = check_token_transport("http://svc:s3cret@runner.example:3000/api?key=t0ken#f")
            .expect_err("plain http to a remote host must be refused");
        let msg = format!("{err:#}");
        assert!(msg.contains("runner.example:3000"), "the host should be named: {msg}");
        for secret in ["s3cret", "svc", "t0ken"] {
            assert!(!msg.contains(secret), "{secret} leaked into: {msg}");
        }
        let err = check_token_transport("http//svc:s3cret@no-scheme").expect_err("invalid URL");
        assert!(!format!("{err:#}").contains("s3cret"), "{err:#}");
    }

    /// Verifies: SRS-IF-010
    #[test]
    fn client_refuses_a_token_over_plain_http_to_a_remote_host() {
        let err = ApiClient::new("http://runner.example", Some("tok"), ClientOptions::default())
            .expect_err("a token over remote plain http must be refused");
        assert!(format!("{err:#}").contains("SRS-IF-010"), "{err:#}");
        // Without a token, plain http to any host stays allowed.
        assert!(ApiClient::new("http://runner.example", None, ClientOptions::default()).is_ok());
    }

    /// Verifies: SRS-IF-006
    #[test]
    fn client_builds_without_token() {
        let client = ApiClient::new("http://localhost:9999", None, ClientOptions::default())
            .expect("client without token should build");
        assert!(client.token.is_none());
    }

    /// Verifies: SRS-PERF-003, SRS-FUNC-020
    #[test]
    fn client_builds_with_retry_and_poll_params() {
        let options = ClientOptions {
            retry_max: 5,
            retry_backoff_initial_secs: 2,
            poll_interval_initial_secs: 3,
            poll_interval_max_secs: 45,
            request_timeout_secs: 7,
        };
        let client = ApiClient::new("http://localhost:9999", Some("tok"), options)
            .expect("client should build");
        assert_eq!(client.options.retry_max, 5);
        assert_eq!(client.options.retry_backoff_initial_secs, 2);
        // SRS-PERF-003: poll cadence must come from config, not be hardcoded.
        assert_eq!(client.options.poll_interval_initial_secs, 3);
        assert_eq!(client.options.poll_interval_max_secs, 45);
        assert_eq!(client.options.request_timeout_secs, 7);
    }

    // -----------------------------------------------------------------------
    // SRS-IF-007: response schema validation
    // -----------------------------------------------------------------------

    fn make_job_status_resp(job_id: &str) -> sqt_types::JobStatusResponse {
        sqt_types::JobStatusResponse {
            job_id: job_id.to_owned(),
            use_case: sqt_types::WorkloadType::default(),
            mode: sqt_types::MeasurementMode::default(),
            repetitions: 1,
            status: sqt_types::JobStatus::Accepted,
            queue_position: None,
            created_at: "2026-01-01T00:00:00Z".to_owned(),
            started_at: None,
            completed_at: None,
            error: None,
            tags: None,
            hardware: None,
            metrics: sqt_types::JobMetrics::default(),
            output: None,
        }
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn validate_job_status_rejects_empty_job_id() {
        let resp = make_job_status_resp("");
        let err = validate_job_status(&resp).unwrap_err();
        assert!(err.to_string().contains("job_id"), "error should mention job_id: {err}");
    }

    /// Verifies: SRS-IF-007
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

    /// Verifies: SRS-IF-007
    #[test]
    fn validate_version_response_rejects_empty_api_version() {
        let resp = make_version_resp("", "1.0.0");
        let err = validate_version_response(&resp).unwrap_err();
        assert!(err.to_string().contains("api_version"), "error should mention api_version: {err}");
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn validate_version_response_rejects_empty_runner_version() {
        let resp = make_version_resp("v1", "");
        let err = validate_version_response(&resp).unwrap_err();
        assert!(
            err.to_string().contains("runner_version"),
            "error should mention runner_version: {err}"
        );
    }

    /// Verifies: SRS-IF-007
    #[test]
    fn validate_version_response_accepts_valid_response() {
        let resp = make_version_resp("v1", "1.2.3");
        assert!(validate_version_response(&resp).is_ok());
    }
}
