use anyhow::Result;

use crate::metrics::{CollectorFactory, MetricCollector, MetricValue, Target};

/// Collect the number of open CRITICAL issues from `SonarQube`.
///
/// **Note:** uses `reqwest::blocking`; the orchestrator must call `collect`
/// inside `tokio::task::spawn_blocking` to avoid blocking the async runtime.
pub struct SonarCriticalIssuesCollector;

impl MetricCollector for SonarCriticalIssuesCollector {
    fn name(&self) -> &'static str {
        "static.critical_issues"
    }

    fn collect(&self, target: &Target, config: &serde_yaml::Value) -> Result<MetricValue> {
        let server_url = config["server_url"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("'server_url' not specified in constraint config"))?;
        let project_key = config["project_key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("'project_key' not specified in constraint config"))?;

        let token = config["token_env"].as_str().and_then(|env_var| std::env::var(env_var).ok());

        let url = format!(
            "{server_url}/api/issues/search?componentKeys={project_key}&severities=CRITICAL&resolved=false"
        );

        let client = reqwest::blocking::Client::new();
        let mut request = client.get(&url);
        if let Some(t) = token {
            request = request.bearer_auth(t);
        }
        let response = request.send()?;
        let body = response.text()?;

        let json_path = target.working_dir.join("sonar_critical_issues.json");
        std::fs::write(&json_path, &body)?;

        let count = parse_issue_count(&body)?;
        Ok(MetricValue::new(count, ""))
    }
}

#[allow(dead_code)]
fn parse_issue_count(json: &str) -> Result<f64> {
    let v: serde_json::Value = serde_json::from_str(json)?;
    v["total"]
        .as_f64()
        .ok_or_else(|| anyhow::anyhow!("missing 'total' field in SonarQube response"))
}

fn create() -> Box<dyn MetricCollector> {
    Box::new(SonarCriticalIssuesCollector)
}

inventory::submit! {
    CollectorFactory { name: "static.critical_issues", create }
}

#[cfg(test)]
#[expect(
    clippy::float_cmp,
    reason = "values are parsed or converted exactly; tests assert exact results"
)]
mod tests {
    use super::*;

    #[test]
    fn name_is_static_critical_issues() {
        assert_eq!(SonarCriticalIssuesCollector.name(), "static.critical_issues");
    }

    #[test]
    fn parse_issue_count_zero() {
        let json = r#"{"total": 0, "issues": []}"#;
        assert_eq!(parse_issue_count(json).unwrap(), 0.0);
    }

    #[test]
    fn parse_issue_count_nonzero() {
        let json = r#"{"total": 7, "issues": []}"#;
        assert_eq!(parse_issue_count(json).unwrap(), 7.0);
    }

    #[test]
    fn parse_issue_count_missing_total() {
        let json = r#"{"issues": []}"#;
        assert!(parse_issue_count(json).is_err());
    }

    #[test]
    fn missing_server_url_returns_error() {
        use crate::metrics::Platform;
        use std::path::PathBuf;
        use std::time::Duration;
        let target = crate::metrics::Target {
            binary: PathBuf::from("/bin/true"),
            platform: Platform::Local,
            input: None,
            working_dir: PathBuf::from("."),
            timeout: Duration::from_secs(10),
        };
        let result = SonarCriticalIssuesCollector.collect(&target, &serde_yaml::Value::Null);
        assert!(result.is_err());
    }
}
