//! Selects KPH inputs and snapshots approved bytes before process execution.
use crate::DeviceIdsError;
use anyhow::{Context, Result};
use duck_core::Context as RunContext;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};

const MAX_EXECUTABLE: u64 = 4 * 1024 * 1024;

#[derive(Deserialize)]
struct Manifest {
    size: usize,
    sha256: String,
}

pub(crate) struct Executable {
    pub source: PathBuf,
    pub bytes: Vec<u8>,
    pub digest: String,
}

pub(crate) fn select(ctx: &RunContext) -> Result<Executable> {
    for path in [
        ctx.paths.root.join("kph_patched"),
        ctx.sysroot.path("/data/local/tmp/kph_patched"),
    ] {
        if !path.try_exists()? {
            continue;
        }
        let bytes = read(&path)?;
        validate_patched(&bytes).with_context(|| format!("validate {}", path.display()))?;
        return Ok(Executable {
            digest: hex::encode(Sha256::digest(&bytes)),
            source: path,
            bytes,
        });
    }
    for path in [
        ctx.sysroot.path("/vendor/bin/kph"),
        ctx.sysroot.path("/system/bin/kph"),
    ] {
        if !path.try_exists()? {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.mode() & 0o022 != 0
            || metadata.mode() & 0o111 == 0
            || (cfg!(target_os = "android") && metadata.uid() != 0)
        {
            return Err(DeviceIdsError::InvalidExecutable(path.display().to_string()).into());
        }
        let bytes = read(&path)?;
        validate_elf(&bytes)?;
        return Ok(Executable {
            digest: hex::encode(Sha256::digest(&bytes)),
            source: path,
            bytes,
        });
    }
    Err(
        DeviceIdsError::Unsupported("no bundled, approved staged or vendor KPH executable found")
            .into(),
    )
}

fn read(path: &PathBuf) -> Result<Vec<u8>> {
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() > MAX_EXECUTABLE {
        return Err(DeviceIdsError::InvalidExecutable(path.display().to_string()).into());
    }
    let mut bytes = vec![];
    file.take(MAX_EXECUTABLE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_EXECUTABLE {
        return Err(DeviceIdsError::InvalidExecutable(path.display().to_string()).into());
    }
    Ok(bytes)
}

pub(crate) fn validate_patched(bytes: &[u8]) -> Result<()> {
    let manifest: Manifest =
        serde_json::from_str(include_str!("../../../module/kph-metadata.json"))?;
    if bytes.len() != manifest.size || hex::encode(Sha256::digest(bytes)) != manifest.sha256 {
        return Err(
            DeviceIdsError::InvalidExecutable("patched sample hash mismatch".into()).into(),
        );
    }
    validate_elf(bytes)
}

fn validate_elf(bytes: &[u8]) -> Result<()> {
    if bytes.len() < 20
        || &bytes[..6] != b"\x7fELF\x02\x01"
        || bytes[18..20] != 183_u16.to_le_bytes()
    {
        return Err(
            DeviceIdsError::InvalidExecutable("expected Android AArch64 ELF".into()).into(),
        );
    }
    Ok(())
}

pub(crate) fn snapshot(executable: &Executable, directory: &std::path::Path) -> Result<PathBuf> {
    let path = directory.join("kph");
    fs::write(&path, &executable.bytes)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    Ok(path)
}
