use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceEntry {
    pub job_id: String,
    pub label: String,
    pub path: PathBuf,
}

pub struct EvidenceStore {
    base_dir: PathBuf,
    entries: Vec<EvidenceEntry>,
}

impl EvidenceStore {
    /// Create a new store rooted at `base_dir`. Creates the directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory cannot be created.
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)
            .with_context(|| format!("cannot create evidence dir: {}", base_dir.display()))?;
        Ok(Self { base_dir, entries: Vec::new() })
    }

    /// Write raw bytes to evidence/`job_id`/`label`.
    ///
    /// # Errors
    ///
    /// Returns an error if the directory or file cannot be created/written.
    pub fn store(&mut self, job_id: &str, label: &str, content: &[u8]) -> Result<PathBuf> {
        let dir = self.base_dir.join(job_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(label);
        std::fs::write(&path, content)
            .with_context(|| format!("cannot write evidence file: {}", path.display()))?;
        self.entries.push(EvidenceEntry {
            job_id: job_id.to_string(),
            label: label.to_string(),
            path: path.clone(),
        });
        Ok(path)
    }

    /// Convenience: write UTF-8 text.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`Self::store`] call fails.
    pub fn store_text(&mut self, job_id: &str, label: &str, text: &str) -> Result<PathBuf> {
        self.store(job_id, label, text.as_bytes())
    }

    /// Convenience: write JSON-serialised value.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialisation fails or the underlying [`Self::store`] call fails.
    pub fn store_json<T: Serialize>(
        &mut self,
        job_id: &str,
        label: &str,
        value: &T,
    ) -> Result<PathBuf> {
        let json =
            serde_json::to_string_pretty(value).context("cannot serialise evidence to JSON")?;
        self.store(job_id, label, json.as_bytes())
    }

    /// Return all stored evidence entries.
    pub fn manifest(&self) -> &[EvidenceEntry] {
        &self.entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Verifies: SRS-FUNC-008
    #[test]
    fn store_creates_file_and_returns_correct_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().join("evidence");
        let mut ev = EvidenceStore::new(base.clone()).expect("new store");

        let path = ev.store("job-123", "stdout.txt", b"hello world").expect("store");

        // Path points inside <base>/job-123/stdout.txt
        assert_eq!(path, base.join("job-123").join("stdout.txt"));
        // File exists with correct content
        let content = fs::read(&path).expect("read file");
        assert_eq!(content, b"hello world");
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn manifest_returns_all_stored_entries() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().join("evidence");
        let mut ev = EvidenceStore::new(base).expect("new store");

        ev.store("job-1", "a.txt", b"aaa").expect("store a");
        ev.store("job-2", "b.txt", b"bbb").expect("store b");
        ev.store("job-1", "c.txt", b"ccc").expect("store c");

        let manifest = ev.manifest();
        assert_eq!(manifest.len(), 3);
        assert_eq!(manifest[0].job_id, "job-1");
        assert_eq!(manifest[0].label, "a.txt");
        assert_eq!(manifest[1].job_id, "job-2");
        assert_eq!(manifest[1].label, "b.txt");
        assert_eq!(manifest[2].job_id, "job-1");
        assert_eq!(manifest[2].label, "c.txt");
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn store_text_writes_utf8_content() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().join("evidence");
        let mut ev = EvidenceStore::new(base).expect("new store");

        let path = ev.store_text("job-x", "note.txt", "hello utf-8 \u{1F600}").expect("store_text");
        let content = fs::read_to_string(&path).expect("read");
        assert_eq!(content, "hello utf-8 \u{1F600}");
    }

    /// Verifies: SRS-FUNC-008
    #[test]
    fn store_json_writes_valid_json() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let base = tmp.path().join("evidence");
        let mut ev = EvidenceStore::new(base).expect("new store");

        let value = serde_json::json!({"status": "ok", "code": 42});
        let path = ev.store_json("job-y", "response.json", &value).expect("store_json");
        let raw = fs::read_to_string(&path).expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&raw).expect("valid json");
        assert_eq!(parsed["status"], "ok");
        assert_eq!(parsed["code"], 42);
    }
}
