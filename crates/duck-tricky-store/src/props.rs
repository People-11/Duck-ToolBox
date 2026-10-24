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

use crate::{
    adapters::ConfigAdapter,
    error::TrickyError,
    model::{ConfigData, PropStatus},
    service::active_adapter,
};

pub const BOOT_HASH_FILE: &str = "/data/adb/boot_hash";
pub const DISABLE_PROP_HANDLER_FILE: &str = "/data/adb/disable_prop_handler";
const VBMETA_DIGEST: &str = "ro.boot.vbmeta.digest";
/// OhMyKeymint signs with its own `vb_hash`, so the boot hash is mirrored into its policy.
const VB_HASH_FIELD: &str = "vb_hash";

#[derive(Debug, Deserialize)]
pub struct PropRequest {
    #[serde(default)]
    pub expected: Option<PropStatus>,
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
    pub restart_required: bool,
}

pub fn status(sysroot: &Sysroot) -> PropStatus {
    PropStatus {
        prop_handler_enabled: !sysroot.path(DISABLE_PROP_HANDLER_FILE).exists(),
        boot_hash: read_boot_hash(sysroot),
    }
}

pub fn save(ctx: &Context, request: PropRequest) -> Result<PropData> {
    let _lock = crate::config_lock::acquire(ctx)?;
    if request
        .expected
        .as_ref()
        .is_some_and(|expected| expected != &status(&ctx.sysroot))
    {
        anyhow::bail!("prop settings changed; refresh before saving");
    }
    // Validate the entire request before changing either configuration file.
    let boot_hash = request
        .boot_hash
        .as_deref()
        .map(str::trim)
        .filter(|hash| !hash.is_empty())
        .map(normalize_boot_hash)
        .transpose()?;
    let sysroot = &ctx.sysroot;
    // A handler-only save must not overwrite a separately configured backend hash.
    let backend = if boot_hash != read_boot_hash(sysroot) {
        boot_hash
            .as_deref()
            .map(|hash| prepare_backend_vb_hash(sysroot, hash))
            .transpose()?
            .flatten()
    } else {
        None
    };
    let disable_path = sysroot.path(DISABLE_PROP_HANDLER_FILE);
    let hash_path = sysroot.path(BOOT_HASH_FILE);
    let mut paths = vec![disable_path.clone(), hash_path.clone()];
    if let Some((adapter, _)) = &backend {
        paths.extend(
            adapter
                .config_files()
                .iter()
                .map(|name| sysroot.path(format!("{}/{name}", adapter.config_dir()))),
        );
    }
    let synced_backend_policy = backend.is_some();
    let restart_required = request.prop_handler_enabled != status(sysroot).prop_handler_enabled
        || (boot_hash.is_none() && hash_path.exists())
        || synced_backend_policy;
    let applied_boot_hash = crate::config_update::apply(&paths, || -> Result<bool> {
        if let Some((adapter, config)) = backend {
            adapter.write(sysroot, &config)?;
        }
        match &boot_hash {
            Some(hash) => write_string_atomic(&hash_path, hash)?,
            None => remove_config_file(&hash_path)?,
        }
        if request.prop_handler_enabled {
            remove_config_file(&disable_path)?;
        } else {
            write_string_atomic(&disable_path, "")?;
        }
        // Apply live state only after every configuration file was published.
        if let Some(hash) = &boot_hash
            && sysroot.root() == Path::new("/")
        {
            props::reset(VBMETA_DIGEST, hash)?;
            props::rebuild_area(VBMETA_DIGEST);
            return Ok(true);
        }
        Ok(false)
    })?;
    Ok(PropData {
        prop_handler_enabled: request.prop_handler_enabled,
        boot_hash,
        applied_boot_hash,
        synced_backend_policy,
        restart_required,
    })
}

fn remove_config_file(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("remove {}", path.display())),
    }
}

type BackendUpdate = (Box<dyn ConfigAdapter>, ConfigData);

fn prepare_backend_vb_hash(sysroot: &Sysroot, hash: &str) -> Result<Option<BackendUpdate>> {
    let Ok((_, adapter)) = active_adapter(sysroot) else {
        return Ok(None);
    };
    let has_field = adapter
        .policy_schema()
        .default_policy
        .iter()
        .any(|field| field.key == VB_HASH_FIELD);
    if !has_field {
        return Ok(None);
    }
    // AVB accepts SHA-512, while some keystore backends require a 32-byte hash.
    // Validate the backend's own contract before changing shared settings.
    crate::policy::sanitize(
        &adapter.policy_schema(),
        [(VB_HASH_FIELD.to_owned(), hash.to_owned())].into(),
    )?;
    let mut config = adapter.read(sysroot)?;
    if config
        .default_policy
        .get(VB_HASH_FIELD)
        .is_some_and(|value| value == hash)
    {
        return Ok(None);
    }
    config
        .default_policy
        .insert(VB_HASH_FIELD.to_owned(), hash.to_owned());
    Ok(Some((adapter, config)))
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

/// AVB supports SHA-256 and SHA-512 digests, represented as lowercase hex.
fn normalize_boot_hash(hash: &str) -> Result<String> {
    let hash: String = hash.chars().filter(|ch| !ch.is_whitespace()).collect();
    let hash = hash.to_ascii_lowercase();
    if !matches!(hash.len(), 64 | 128) || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TrickyError::InvalidPolicy {
            field: "boot_hash".into(),
            reason: "expected 64 or 128 hexadecimal characters".into(),
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
            normalize_boot_hash(&"CD".repeat(64)).unwrap(),
            "cd".repeat(64)
        );
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
                        expected: None,
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
                expected: None,
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
                expected: None,
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
                    expected: None,
                    prop_handler_enabled: true,
                    boot_hash: None,
                },
            )
            .unwrap_err();
            assert!(error.to_string().contains(&path.display().to_string()));
            assert!(path.is_dir());
        }
    }

    fn omk(ctx: &Context, policy: &str) -> std::path::PathBuf {
        let module = ctx.sysroot.path("/data/adb/modules/oh_my_keymint");
        fs::create_dir_all(&module).unwrap();
        fs::write(
            module.join("module.prop"),
            "id=oh_my_keymint\nversionCode=1\n",
        )
        .unwrap();
        let config = ctx.sysroot.path("/data/misc/keystore/omk/config.toml");
        fs::create_dir_all(config.parent().unwrap()).unwrap();
        fs::write(&config, policy).unwrap();
        config
    }

    #[test]
    fn handler_only_save_preserves_independent_backend_policy() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let policy = "[trust]\nvb_hash=\"random\"\n";
        let config = omk(&ctx, policy);
        let hash = "ab".repeat(32);
        fs::write(ctx.sysroot.path(BOOT_HASH_FILE), &hash).unwrap();
        let result = save(
            &ctx,
            PropRequest {
                expected: None,
                prop_handler_enabled: false,
                boot_hash: Some(hash),
            },
        )
        .unwrap();
        assert!(!result.synced_backend_policy);
        assert_eq!(fs::read_to_string(&config).unwrap(), policy);
    }

    #[test]
    fn backend_errors_are_checked_before_shared_settings_change() {
        for (policy, new_hash) in [
            ("[broken", "cd".repeat(32)),
            ("[trust]\nvb_hash=\"auto\"\n", "cd".repeat(64)),
        ] {
            let root = tempfile::tempdir().unwrap();
            let ctx = context(root.path());
            let config = omk(&ctx, policy);
            let hash_path = ctx.sysroot.path(BOOT_HASH_FILE);
            fs::write(&hash_path, "ab".repeat(32)).unwrap();
            assert!(
                save(
                    &ctx,
                    PropRequest {
                        expected: None,
                        prop_handler_enabled: false,
                        boot_hash: Some(new_hash),
                    }
                )
                .is_err()
            );
            assert_eq!(fs::read_to_string(&config).unwrap(), policy);
            assert_eq!(fs::read_to_string(&hash_path).unwrap(), "ab".repeat(32));
            assert!(!ctx.sysroot.path(DISABLE_PROP_HANDLER_FILE).exists());
        }
    }

    #[test]
    fn partial_backend_write_restores_the_injector_and_shared_settings() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let config = omk(&ctx, "");
        fs::remove_file(&config).unwrap();
        let injector = config.with_file_name("injector.toml");
        let original = "# retain exact formatting\nscoop = [\"com.original\"]\n";
        fs::write(&injector, original).unwrap();
        let hash_path = ctx.sysroot.path(BOOT_HASH_FILE);
        fs::write(&hash_path, "ab".repeat(32)).unwrap();
        let error = save(
            &ctx,
            PropRequest {
                expected: None,
                prop_handler_enabled: false,
                boot_hash: Some("cd".repeat(32)),
            },
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("original configuration files restored")
        );
        assert_eq!(fs::read_to_string(injector).unwrap(), original);
        assert_eq!(fs::read_to_string(hash_path).unwrap(), "ab".repeat(32));
        assert!(!config.exists());
        assert!(!ctx.sysroot.path(DISABLE_PROP_HANDLER_FILE).exists());
    }
    #[test]
    fn stale_prop_form_cannot_replace_external_shared_settings() {
        let root = tempfile::tempdir().unwrap();
        let ctx = context(root.path());
        let previous = status(&ctx.sysroot);
        let path = ctx.sysroot.path(BOOT_HASH_FILE);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "ab".repeat(32)).unwrap();
        let error = save(
            &ctx,
            PropRequest {
                expected: Some(previous),
                prop_handler_enabled: false,
                boot_hash: None,
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("prop settings changed"));
        assert_eq!(fs::read_to_string(path).unwrap(), "ab".repeat(32));
        assert!(!ctx.sysroot.path(DISABLE_PROP_HANDLER_FILE).exists());
    }
}
