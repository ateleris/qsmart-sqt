use std::path::Path;

use anyhow::{Context, Result};

/// Get the size of a binary file in kilobytes.
pub fn collect(binary: &str) -> Result<f64> {
    let path = Path::new(binary);
    let metadata =
        std::fs::metadata(path).with_context(|| format!("binary_size: cannot stat '{binary}'"))?;

    #[allow(clippy::cast_precision_loss)]
    let size_kb = metadata.len() as f64 / 1024.0;
    Ok(size_kb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_on_self() {
        // Use the test binary itself as a target
        let exe = std::env::current_exe().unwrap();
        let size = collect(exe.to_str().unwrap()).unwrap();
        assert!(size > 0.0);
    }
}
