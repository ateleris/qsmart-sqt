use std::time::Duration;

use anyhow::{Context, Result, bail};
use reqwest::header::{AUTHORIZATION, HeaderMap, HeaderValue};

use super::types::{
    AnalysisAccepted, AnalysisRequest, AnalysisStatusResponse, ErrorResponse, RunAccepted,
    RunRequest, RunStatusResponse,
};

pub struct ApiClient {
    client: reqwest::Client,
    base_url: String,
}

impl ApiClient {
    /// Create a new `ApiClient`.
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

    // -----------------------------------------------------------------------
    // Run endpoints
    // -----------------------------------------------------------------------

    /// Submit a run job.
    ///
    /// Returns the `run_id` on success.
    ///
    /// # Errors
    ///
    /// Returns an error on non-2xx responses or network failures.
    pub async fn submit_run(&self, request: &RunRequest) -> Result<String> {
        let url = format!("{}/api/v1/runs", self.base_url);
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

        let accepted: RunAccepted =
            response.json().await.context("Failed to parse RunAccepted response")?;
        Ok(accepted.run_id)
    }

    /// Retrieve the current status of a run.
    ///
    /// # Errors
    ///
    /// Returns an error on non-2xx responses or network failures.
    pub async fn get_run_status(&self, run_id: &str) -> Result<RunStatusResponse> {
        let url = format!("{}/api/v1/runs/{run_id}", self.base_url);
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

        response.json().await.context("Failed to parse RunStatusResponse")
    }

    /// Poll until the run reaches a terminal state (`complete` or `failed`).
    ///
    /// Uses exponential backoff: starts at 1 s, doubles each attempt, caps at 30 s.
    ///
    /// # Errors
    ///
    /// Returns an error if any poll request fails.
    pub async fn wait_for_run(&self, run_id: &str) -> Result<RunStatusResponse> {
        const MAX_DELAY: Duration = Duration::from_secs(30);
        let mut delay = Duration::from_secs(1);

        loop {
            let status = self.get_run_status(run_id).await?;
            tracing::debug!(run_id, status = ?status.status, "polled run status");

            if status.status.is_terminal() {
                return Ok(status);
            }

            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(MAX_DELAY);
        }
    }

    // -----------------------------------------------------------------------
    // Analysis endpoints
    // -----------------------------------------------------------------------

    /// Submit an analysis job.
    ///
    /// Returns the `analysis_id` on success.
    ///
    /// # Errors
    ///
    /// Returns an error on non-2xx responses or network failures.
    pub async fn submit_analysis(&self, request: &AnalysisRequest) -> Result<String> {
        let url = format!("{}/api/v1/analysis", self.base_url);
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

        let accepted: AnalysisAccepted =
            response.json().await.context("Failed to parse AnalysisAccepted response")?;
        Ok(accepted.analysis_id)
    }

    /// Retrieve the current status of an analysis.
    ///
    /// # Errors
    ///
    /// Returns an error on non-2xx responses or network failures.
    pub async fn get_analysis_status(&self, analysis_id: &str) -> Result<AnalysisStatusResponse> {
        let url = format!("{}/api/v1/analysis/{analysis_id}", self.base_url);
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

        response.json().await.context("Failed to parse AnalysisStatusResponse")
    }

    /// Poll until the analysis reaches a terminal state (`complete` or `failed`).
    ///
    /// Uses exponential backoff: starts at 1 s, doubles each attempt, caps at 30 s.
    ///
    /// # Errors
    ///
    /// Returns an error if any poll request fails.
    pub async fn wait_for_analysis(&self, analysis_id: &str) -> Result<AnalysisStatusResponse> {
        const MAX_DELAY: Duration = Duration::from_secs(30);
        let mut delay = Duration::from_secs(1);

        loop {
            let status = self.get_analysis_status(analysis_id).await?;
            tracing::debug!(analysis_id, status = ?status.status, "polled analysis status");

            if status.status.is_terminal() {
                return Ok(status);
            }

            tokio::time::sleep(delay).await;
            delay = (delay * 2).min(MAX_DELAY);
        }
    }
}
