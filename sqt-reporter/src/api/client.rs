use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use sqt_types::{ErrorResponse, JobAccepted, JobRequest, JobStatus, JobStatusResponse};

pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    /// Create a new `ApiClient`.
    ///
    /// If `token` is `Some`, it is sent as a `Bearer` token in every request.
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

    // -----------------------------------------------------------------------
    // Job endpoint  (POST /api/v1/jobs  +  GET /api/v1/jobs/{job_id})
    // -----------------------------------------------------------------------

    /// Submit a qualification job.
    ///
    /// Returns the `job_id` on success (202 Accepted).
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
                bail!("API error {status}: {} - {}", err.error.code, err.error.message);
            }
            bail!("API returned {status}: {body}");
        }

        let accepted: JobAccepted =
            response.json().await.context("Failed to parse JobAccepted response")?;
        Ok(accepted.job_id)
    }

    /// Retrieve the current status of a job.
    pub async fn get_job_status(&self, job_id: &str) -> Result<JobStatusResponse> {
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
                bail!("API error {status}: {} - {}", err.error.code, err.error.message);
            }
            bail!("API returned {status}: {body}");
        }

        response.json().await.context("Failed to parse JobStatusResponse")
    }

    /// Poll until the job reaches `complete`, or fail fast on any terminal error.
    ///
    /// Uses exponential backoff: 1 s initial, ×2 per poll, 30 s cap.
    /// Bails immediately when the server reports `failed`, embedding the
    /// server-side error field. Bails with a timeout error after `timeout`.
    ///
    /// # Errors
    ///
    /// Returns an error if a poll request fails, the job transitions to `failed`,
    /// or `timeout` is exceeded before the job completes.
    pub async fn wait_for_job(&self, job_id: &str, timeout: Duration) -> Result<JobStatusResponse> {
        const MAX_DELAY: Duration = Duration::from_secs(30);
        let deadline = Instant::now() + timeout;
        let mut delay = Duration::from_secs(1);

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
            delay = (delay * 2).min(MAX_DELAY);
        }
    }
}
