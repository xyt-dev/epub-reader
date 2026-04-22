use anyhow::{Context, Result};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Atomically replace a file by writing to a sibling temp file first.
pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent dir '{}'", parent.display()))?;
    }

    let tmp_path = temp_path(path);
    let mut file = File::create(&tmp_path)
        .with_context(|| format!("failed to create temp file '{}'", tmp_path.display()))?;

    file.write_all(content)
        .with_context(|| format!("failed to write temp file '{}'", tmp_path.display()))?;
    file.sync_all()
        .with_context(|| format!("failed to sync temp file '{}'", tmp_path.display()))?;

    std::fs::rename(&tmp_path, path).with_context(|| {
        format!(
            "failed to rename temp file '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )
    })?;

    Ok(())
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path
        .file_name()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| "tmp".into());
    name.push(".tmp");
    path.with_file_name(name)
}
