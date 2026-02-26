use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::types::{ErrorResponse, JobAccepted, JobRequest, JobStatusResponse};

pub struct JobApiClient {
    client: reqwest::Client,
    base_url: String,
}

impl JobApiClient {
    /// Create a new `JobApiClient`.
    ///
    /// If `token` is `Some`, it is sent as a `Bearer` token in every request.
    ///
    /// # Errors
    ///
    /// Returns an error if the bearer token contains non-ASCII characters or
    /// if the underlying HTTP client cannot be constructed.
    pub fn new(base_url: &str, token: Option<&str>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        if let Some(tok) = token {
            let value = HeaderValue::from_str(&format!("Bearer {tok}"))
                .context("API token contains invalid header characters")?;
            headers.insert(AUTHORIZATION, value);
        }
        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to build HTTP client")?;
        Ok(Self { client, base_url: base_url.trim_end_matches('/').to_owned() })
    }

    /// Submit a qualification job.
    ///
    /// Returns the `job_id` on success.
    ///
    /// # Errors
    ///
    /// Returns an error on non-2xx responses or network failures.
    pub async fn submit_job(&self, request: &JobRequest) -> Result<String> {
        let url = format!("{}/api/v1/jobs", self.base_url);
        let response = self
            .client
            .post(&url)
            .json(request)
            .send()
            .await
            .with_context(|| format!("POST {url} failed"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("API error {status}: {} — {}", err.error.code, err.error.message);
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
    /// Returns an error on non-2xx responses or network failures.
    pub async fn get_status(&self, job_id: &str) -> Result<JobStatusResponse> {
        let url = format!("{}/api/v1/jobs/{job_id}", self.base_url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .with_context(|| format!("GET {url} failed"))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            if let Ok(err) = serde_json::from_str::<ErrorResponse>(&body) {
                bail!("API error {status}: {} — {}", err.error.code, err.error.message);
            }
            bail!("API returned {status}: {body}");
        }

        response.json().await.context("Failed to parse JobStatusResponse")
    }

    /// Poll until the job reaches a terminal state (`complete` or `failed`).
    ///
    /// Uses exponential backoff: starts at 1 s, doubles each attempt, caps at 30 s.
    ///
    /// # Errors
    ///
    /// Returns an error if any poll request fails.
    pub async fn wait_for_completion(&self, job_id: &str) -> Result<JobStatusResponse> {
        const MAX_DELAY: Duration = Duration::from_secs(30);
        let mut delay = Duration::from_secs(1);

        loop {
            let status = self.get_status(job_id).await?;
            tracing::debug!(
                job_id,
                status = ?status.status,
                "polled job status"
            );

            if status.status.is_terminal() {
                return Ok(status);
            }

            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(MAX_DELAY);
        }
    }
}
