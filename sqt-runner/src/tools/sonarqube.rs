use anyhow::{Context, Result, bail};

/// Query the `SonarQube` API for critical and major issues.
///
/// Uses the `api/issues/search` endpoint with component key derived from
/// the project name (typically the binary name).
pub fn collect(
    sonar_url: &str,
    sonar_token: &str,
    project_key: &str,
) -> Result<(i64, i64)> {
    let critical = fetch_issue_count(sonar_url, sonar_token, project_key, "CRITICAL")?;
    let major = fetch_issue_count(sonar_url, sonar_token, project_key, "MAJOR")?;
    Ok((critical, major))
}

fn fetch_issue_count(
    sonar_url: &str,
    sonar_token: &str,
    project_key: &str,
    severity: &str,
) -> Result<i64> {
    let url = format!(
        "{}/api/issues/search?componentKeys={}&severities={}&resolved=false&ps=1",
        sonar_url.trim_end_matches('/'),
        project_key,
        severity,
    );

    let client = reqwest::blocking::Client::new();
    let resp = client
        .get(&url)
        .basic_auth(sonar_token, Option::<&str>::None)
        .send()
        .context("sonarqube: failed to connect to SonarQube API")?;

    if !resp.status().is_success() {
        bail!(
            "sonarqube: API returned status {} for severity {}",
            resp.status(),
            severity
        );
    }

    let body: serde_json::Value = resp
        .json()
        .context("sonarqube: failed to parse JSON response")?;

    body["total"]
        .as_i64()
        .context("sonarqube: 'total' field missing or not an integer")
}

#[cfg(test)]
mod tests {
    // Integration tests for SonarQube require a running instance,
    // so we only test the parsing logic here would be manual.
    // The module is tested end-to-end via the dev server.
}
