//! Prop settings: the sensitive-prop handler and the Verified Boot hash.
//!
//! File locations match Tricky Addon (`/data/adb/boot_hash`,
//! `/data/adb/disable_prop_handler`) so both tools share one configuration. Props are
//! changed with `resetprop -n`, which the KernelSU module guide requires in boot scripts.

use std::{io::ErrorKind, path::Path};

use anyhow::{Context as _, Result};
use duck_core::{Context, Sysroot, fs::write_string_atomic};
use duck_platform::props;
use serde::{Deserialize, Serialize};

use crate::{error::TrickyError, model::PropStatus, service::active_adapter};

pub const BOOT_HASH_FILE: &str = "/data/adb/boot_hash";
pub const DISABLE_PROP_HANDLER_FILE: &str = "/data/adb/disable_prop_handler";
const VBMETA_DIGEST: &str = "ro.boot.vbmeta.digest";
/// OhMyKeymint signs with its own `vb_hash`, so the boot hash is mirrored into its policy.
const VB_HASH_FIELD: &str = "vb_hash";

#[derive(Debug, Deserialize)]
pub struct PropRequest {
    pub prop_handler_enabled: bool,
    #[serde(default)]
    pub boot_hash: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PropData {
    pub prop_handler_enabled: bool,
    pub boot_hash: Option<String>,
    pub applied_boot_hash: bool,
    /// Whether the active backend's `vb_hash` policy was updated to the new hash.
    pub synced_backend_policy: bool,
}

pub fn status(sysroot: &Sysroot) -> PropStatus {
    PropStatus {
        prop_handler_enabled: !sysroot.path(DISABLE_PROP_HANDLER_FILE).exists(),
        boot_hash: read_boot_hash(sysroot),
    }
}

pub fn save(ctx: &Context, request: PropRequest) -> Result<PropData> {
    // Validate the entire request before changing either configuration file.
    let boot_hash = request
        .boot_hash
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(normalize_boot_hash)
        .transpose()?;
    let sysroot = &ctx.sysroot;
    let disable_path = sysroot.path(DISABLE_PROP_HANDLER_FILE);
    if request.prop_handler_enabled {
        remove_config_file(&disable_path)?;
    } else {
        write_string_atomic(&disable_path, "")?;
    }

    let mut applied_boot_hash = false;
    let mut synced_backend_policy = false;
    match &boot_hash {
        Some(hash) => {
            write_string_atomic(&sysroot.path(BOOT_HASH_FILE), hash)?;
            // Only touch live properties when managing the running system, not a test sysroot.
            if sysroot.root() == std::path::Path::new("/") {
                props::reset(VBMETA_DIGEST, hash)?;
                props::rebuild_area(VBMETA_DIGEST);
                applied_boot_hash = true;
            }
            synced_backend_policy = sync_backend_vb_hash(sysroot, hash)?;
        }
        None => {
            remove_config_file(&sysroot.path(BOOT_HASH_FILE))?;
        }
    }

    Ok(PropData {
        prop_handler_enabled: request.prop_handler_enabled,
        boot_hash,
        applied_boot_hash,
        synced_backend_policy,
    })
}

fn remove_config_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

fn sync_backend_vb_hash(sysroot: &Sysroot, hash: &str) -> Result<bool> {
    let Ok((_, adapter)) = active_adapter(sysroot) else {
        return Ok(false);
    };
    let has_field = adapter
        .policy_schema()
        .default_policy
        .iter()
        .any(|field| field.key == VB_HASH_FIELD);
    if !has_field {
        return Ok(false);
    }
    let mut config = adapter.read(sysroot)?;
    config
        .default_policy
        .insert(VB_HASH_FIELD.to_owned(), hash.to_owned());
    adapter.write(sysroot, &config)?;
    Ok(true)
}

fn read_boot_hash(sysroot: &Sysroot) -> Option<String> {
    let raw = duck_core::fs::read_optional(&sysroot.path(BOOT_HASH_FILE)).ok()??;
    let hash: String = raw
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(str::chars)
        .filter(|ch| !ch.is_whitespace())
        .collect();
    (!hash.is_empty()).then(|| hash.to_ascii_lowercase())
}

/// The `verifiedBootHash` from KeyAttestation is a 64-character lowercase hex string.
fn normalize_boot_hash(hash: &str) -> Result<String> {
    let hash: String = hash.chars().filter(|ch| !ch.is_whitespace()).collect();
    let hash = hash.to_ascii_lowercase();
    if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TrickyError::InvalidPolicy {
            field: "boot_hash".into(),
            reason: "expected 64 hexadecimal characters".into(),
        }
        .into());
    }
    Ok(hash)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use duck_core::{AppPaths, Context, Sysroot};

    use super::{
        BOOT_HASH_FILE, DISABLE_PROP_HANDLER_FILE, PropRequest, normalize_boot_hash,
        read_boot_hash, save, status,
    };

    fn context(root: &std::path::Path) -> Context {
        Context {
            paths: AppPaths::for_root(&root.join("module"), &root.join("runtime")),
            sysroot: Sysroot::new(root.join("sys")),
        }
    }

    #[test]
    fn normalizes_and_validates_boot_hash() {
        assert_eq!(
            normalize_boot_hash(&format!(" {} ", "AB".repeat(32))).unwrap(),
            "ab".repeat(32)
        );
        assert!(normalize_boot_hash("abc").is_err());
        assert!(normalize_boot_hash(&"z".repeat(64)).is_err());
        assert_eq!(
            normalize_boot_hash(&format!("{} \n {}", "AB".repeat(16), "CD".repeat(16))).unwrap(),
            format!("{}{}", "ab".repeat(16), "cd".repeat(16))
        );
    }

    #[test]
    fn status_reflects_disable_file() {
        let root = std::env::temp_dir().join(format!("duck-props-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sysroot = Sysroot::new(&root);
        assert!(status(&sysroot).prop_handler_enabled);

        let disable = sysroot.path(DISABLE_PROP_HANDLER_FILE);
        fs::create_dir_all(disable.parent().unwrap()).unwrap();
        fs::write(&disable, "").unwrap();
        assert!(!status(&sysroot).prop_handler_enabled);
    }

    #[test]
    fn reads_boot_hash_ignoring_comments() {
        let root = std::env::temp_dir().join(format!("duck-props-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let sysroot = Sysroot::new(&root);
        let path = sysroot.path(super::BOOT_HASH_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("# note\n{}\n", "CD".repeat(32))).unwrap();

        assert_eq!(read_boot_hash(&sysroot), Some("cd".repeat(32)));
    }

    #[test]
    fn invalid_hash_does_not_change_the_handler_or_saved_hash() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let hash_path = ctx.sysroot.path(BOOT_HASH_FILE);
        let disable = ctx.sysroot.path(DISABLE_PROP_HANDLER_FILE);
        fs::create_dir_all(hash_path.parent().unwrap()).unwrap();
        fs::write(&hash_path, "ab".repeat(32)).unwrap();

        for was_enabled in [true, false] {
            if !was_enabled {
                fs::write(&disable, "").unwrap();
            }
            assert!(
                save(
                    &ctx,
                    PropRequest {
                        prop_handler_enabled: !was_enabled,
                        boot_hash: Some("invalid".into()),
                    },
                )
                .is_err()
            );
            assert_eq!(status(&ctx.sysroot).prop_handler_enabled, was_enabled);
            assert_eq!(fs::read_to_string(&hash_path).unwrap(), "ab".repeat(32));
        }
    }

    #[test]
    fn saves_formatted_hash_independently_of_the_handler_and_clears_it() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let result = save(
            &ctx,
            PropRequest {
                prop_handler_enabled: false,
                boot_hash: Some(format!("{} \n {}", "AB".repeat(16), "AB".repeat(16))),
            },
        )
        .unwrap();
        assert!(!result.prop_handler_enabled);
        assert!(!result.applied_boot_hash);
        assert_eq!(read_boot_hash(&ctx.sysroot), Some("ab".repeat(32)));

        let result = save(
            &ctx,
            PropRequest {
                prop_handler_enabled: true,
                boot_hash: Some(" \n ".into()),
            },
        )
        .unwrap();
        assert!(result.prop_handler_enabled);
        assert!(result.boot_hash.is_none());
        assert!(!ctx.sysroot.path(BOOT_HASH_FILE).exists());
        assert!(!ctx.sysroot.path(DISABLE_PROP_HANDLER_FILE).exists());
    }

    #[test]
    fn failed_config_removal_is_reported() {
        for file in [DISABLE_PROP_HANDLER_FILE, BOOT_HASH_FILE] {
            let root = tempfile::tempdir().unwrap();
            let ctx = context(root.path());
            let path = ctx.sysroot.path(file);
            fs::create_dir_all(&path).unwrap();
            let error = save(
                &ctx,
                PropRequest {
                    prop_handler_enabled: true,
                    boot_hash: None,
                },
            )
            .unwrap_err();
            assert!(error.to_string().contains(&path.display().to_string()));
            assert!(path.is_dir());
        }
    }
}
