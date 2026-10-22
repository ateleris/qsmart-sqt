// SPDX-FileCopyrightText: 2026 Ateleris GmbH
// SPDX-License-Identifier: ESA-PL-permissive-2.4

use std::path::Path;

use anyhow::Context;

pub use schema::{RunnerConfig, SqtConfig};

mod schema;

/// Load and validate the SQT configuration from a YAML file.
///
/// # Errors
///
/// Returns an error if the file cannot be read, the YAML is invalid,
/// or the config fails validation.
pub fn load_config(path: &Path) -> anyhow::Result<SqtConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file: {}", path.display()))?;
    let mut config: SqtConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse config file: {}", path.display()))?;
    let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
    config.validate(config_dir)?;
    Ok(config)
}
