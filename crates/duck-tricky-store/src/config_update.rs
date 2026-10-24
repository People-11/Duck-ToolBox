//! Roll back ordinary keystore updates on I/O failure while retaining file metadata.
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use duck_core::{
    Context,
    fs::{read_optional, write_bytes_preserving},
};

use crate::adapters::ConfigAdapter;

pub(crate) fn paths(ctx: &Context, adapter: &dyn ConfigAdapter) -> Vec<PathBuf> {
    adapter
        .config_files()
        .iter()
        .map(|name| ctx.sysroot.path(format!("{}/{name}", adapter.config_dir())))
        .collect()
}

pub(crate) fn apply<T>(paths: &[PathBuf], publish: impl FnOnce() -> Result<T>) -> Result<T> {
    let before = paths
        .iter()
        .map(|path| Ok((path, read_optional(path)?)))
        .collect::<Result<Vec<_>>>()?;
    match publish() {
        Ok(value) => Ok(value),
        Err(error) => {
            let mut failures = Vec::new();
            for (path, content) in before {
                // Do not rewrite an untouched file or disturb its daemon watcher.
                if read_optional(path).is_ok_and(|current| current == content) {
                    continue;
                }
                let result = match content {
                    Some(content) => write_bytes_preserving(path, content.as_bytes()),
                    None => std::fs::remove_file(path)
                        .with_context(|| format!("remove {}", path.display())),
                };
                if let Err(error) = result {
                    failures.push(format!("{}: {error:#}", path.display()));
                }
            }
            if !failures.is_empty() {
                return Err(error.context(format!(
                    "settings recovery incomplete: {}",
                    failures.join("; ")
                )));
            }
            Err(error.context("settings save failed; original configuration files restored"))
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn failed_publication_restores_existing_files_and_removes_new_files() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("config.toml");
        let created = root.path().join("injector.toml");
        std::fs::write(&original, "original").unwrap();
        let result: anyhow::Result<()> = super::apply(&[original.clone(), created.clone()], || {
            std::fs::write(&original, "changed")?;
            std::fs::write(&created, "new")?;
            anyhow::bail!("state publication failed")
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("original configuration files restored")
        );
        assert_eq!(std::fs::read_to_string(original).unwrap(), "original");
        assert!(!created.exists());
    }
}
