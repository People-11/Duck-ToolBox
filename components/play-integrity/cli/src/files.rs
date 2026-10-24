// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: private atomic storage, preserved external directories and guarded recovery.
use anyhow::{Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
};

#[derive(Debug)]
pub struct RecoveryError(pub Value);
impl std::fmt::Display for RecoveryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "configuration recovery failed: {}", self.0)
    }
}
impl std::error::Error for RecoveryError {}

pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
pub fn read(path: &Path) -> Result<String> {
    use std::io::Read;
    if !path.exists() {
        return Ok(String::new());
    }
    let mut value = String::new();
    fs::File::open(path)?
        .take(128 * 1024 + 1)
        .read_to_string(&mut value)?;
    if value.len() > 128 * 1024 {
        bail!("file exceeds 128 KiB: {}", path.display());
    }
    Ok(value)
}
pub fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("no parent"))?;
    // A basename-only output (e.g. wget -O PIXEL_VERSIONS_HTML) has an empty
    // parent. Resolve it to the current directory for create and fsync.
    let parent = if parent.as_os_str().is_empty() {
        Path::new(".")
    } else {
        parent
    };
    // Output may belong to an external migration/download directory. Keep its mode;
    // only newly created directories and the new file need private permissions.
    let mut directories = fs::DirBuilder::new();
    directories.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        directories.mode(0o700);
    }
    directories.create(parent)?;
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)?;
        fs::File::open(parent)?.sync_all()?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}
pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    atomic(path, &serde_json::to_vec_pretty(value)?)
}
pub fn json_file(path: &Path) -> Result<Value> {
    let raw = read(path)?;
    Ok(if raw.is_empty() {
        json!({})
    } else {
        serde_json::from_str(&raw)?
    })
}
pub struct Lock {
    _file: fs::File,
}
impl Drop for Lock {
    fn drop(&mut self) {
        // A child forked by another thread may briefly retain this file description
        // before exec closes it. Lock ownership ends with this guard, not that copy.
        let _ = self._file.unlock();
    }
}
impl Lock {
    pub fn acquire(data: &Path) -> Result<Self> {
        private_dir(data)?;
        let path = data.join("write.lock");
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(path)?;
        file.try_lock()
            .map_err(|error| anyhow::anyhow!("engine is busy: {error}"))?;
        Ok(Self { _file: file })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn atomic_private_and_bounded() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("config");
        atomic(&path, b"old").unwrap();
        atomic(&path, b"new").unwrap();
        assert_eq!(read(&path).unwrap(), "new");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, vec![b'x'; 128 * 1024 + 1]).unwrap();
        assert!(read(&path).is_err());
        let lock = Lock::acquire(root.path()).unwrap();
        assert!(Lock::acquire(root.path()).is_err());
        drop(lock);
        assert!(Lock::acquire(root.path()).is_ok());
    }

    #[test]
    fn atomic_writes_basename_only_paths() {
        // Use a unique name in the current directory without changing cwd
        // globally (which would race with other tests).
        let temporary = tempfile::Builder::new()
            .prefix(".pif-atomic-relative-")
            .tempfile_in(".")
            .unwrap();
        let basename = Path::new(temporary.path().file_name().unwrap());
        assert!(basename.parent().unwrap().as_os_str().is_empty());

        atomic(basename, b"first download").unwrap();
        assert_eq!(fs::read_to_string(basename).unwrap(), "first download");
        atomic(basename, b"updated download").unwrap();
        assert_eq!(fs::read_to_string(basename).unwrap(), "updated download");
    }
    #[cfg(unix)]
    #[test]
    fn lock_guard_releases_even_while_an_inherited_description_exists() {
        let root = tempfile::tempdir().unwrap();
        let lock = Lock::acquire(root.path()).unwrap();
        let inherited = lock._file.try_clone().unwrap();
        drop(lock);
        let next = Lock::acquire(root.path()).unwrap();
        drop(next);
        drop(inherited);
    }
    #[cfg(unix)]
    #[test]
    fn atomic_output_preserves_existing_directories_and_privately_creates_new_ones() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let shared = root.path().join("shared");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o755)).unwrap();
        let path = shared.join("new/nested/config");
        atomic(&path, b"value").unwrap();
        atomic(&shared.join("export.prop"), b"MODEL=Example").unwrap();
        for (path, expected) in [
            (shared.clone(), 0o755),
            (shared.join("new"), 0o700),
            (shared.join("new/nested"), 0o700),
            (path, 0o600),
            (shared.join("export.prop"), 0o600),
        ] {
            assert_eq!(
                fs::metadata(path).unwrap().permissions().mode() & 0o777,
                expected
            );
        }
    }
}
