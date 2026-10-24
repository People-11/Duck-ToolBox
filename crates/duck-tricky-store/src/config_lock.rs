//! Serialize this manager's keystore read/modify/write operations across CLI processes.
use anyhow::{Context as _, Result};
use duck_core::Context;

pub(crate) struct ConfigLock(std::fs::File);

impl Drop for ConfigLock {
    fn drop(&mut self) {
        // A concurrently forked command can briefly inherit this open description.
        // Release ownership now rather than waiting for the child to exec or exit.
        let _ = self.0.unlock();
    }
}

pub(crate) fn acquire(ctx: &Context) -> Result<ConfigLock> {
    std::fs::create_dir_all(&ctx.paths.var_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(ctx.paths.var_dir.join("tricky-store-config.lock"))?;
    file.try_lock()
        .context("keystore configuration is busy; retry after the current operation")?;
    Ok(ConfigLock(file))
}

#[cfg(test)]
mod tests {
    #[test]
    fn concurrent_writer_is_rejected_until_owner_releases_lock() {
        let root = tempfile::tempdir().unwrap();
        let ctx = duck_core::Context {
            paths: duck_core::AppPaths::for_root(root.path(), &root.path().join("runtime")),
            sysroot: duck_core::Sysroot::new(root.path()),
        };
        let first = super::acquire(&ctx).unwrap();
        let inherited = first.0.try_clone().unwrap();
        assert!(super::acquire(&ctx).is_err());
        drop(first);
        assert!(super::acquire(&ctx).is_ok());
        drop(inherited);
    }
}
