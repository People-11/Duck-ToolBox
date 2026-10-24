//! Previewable patch-level updates owned by the keystore configuration adapters.
use crate::{
    model::{Backend, Policy},
    service::active_adapter,
};
use anyhow::{Context as _, Result, bail};
use duck_core::{
    Context,
    fs::{write_bytes_preserving, write_string_atomic},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs};

#[derive(Default, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub patch: String,
    #[serde(default)]
    pub revision: String,
    #[serde(default)]
    pub backup: String,
}
type Snapshot = BTreeMap<String, Option<String>>;
#[derive(Serialize, Deserialize)]
struct Backup {
    backend: Backend,
    before: Snapshot,
    applied_revision: String,
    #[serde(default)]
    after: Option<Snapshot>,
    #[serde(default)]
    restart_required: bool,
}
fn snapshot(ctx: &Context, dir: &str, backend: Backend) -> Result<Snapshot> {
    crate::adapters::for_backend(backend)
        .config_files()
        .iter()
        .map(|name| {
            let path = ctx.sysroot.path(format!("{dir}/{name}"));
            if path.metadata().is_ok_and(|m| m.len() > 1024 * 1024) {
                bail!("configuration too large: {}", path.display());
            }
            Ok(((*name).into(), duck_core::fs::read_optional(&path)?))
        })
        .collect()
}
fn revision(snapshot: &Snapshot) -> Result<String> {
    Ok(hex::encode(Sha256::digest(serde_json::to_vec(snapshot)?)))
}
fn restore_files(ctx: &Context, dir: &str, snapshot: &Snapshot) -> Result<()> {
    let mut failed = Vec::new();
    for (name, content) in snapshot {
        let path = ctx.sysroot.path(format!("{dir}/{name}"));
        let result = if let Some(content) = content {
            write_bytes_preserving(&path, content.as_bytes())
        } else if path.exists() {
            fs::remove_file(&path).map_err(Into::into)
        } else {
            Ok(())
        };
        let result = result.and_then(|()| {
            if let Some(parent) = path.parent().filter(|p| p.exists()) {
                fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        });
        if let Err(error) = result {
            failed.push(format!("{}: {error:#}", path.display()));
        }
    }
    if !failed.is_empty() {
        bail!("restore failed: {}", failed.join("; "));
    }
    Ok(())
}
fn proposed(backend: Backend, patch: &str) -> Result<Policy> {
    let valid = patch.len() == 10
        && patch.as_bytes()[4] == b'-'
        && patch.as_bytes()[7] == b'-'
        && patch
            .chars()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !valid {
        bail!("expected security patch YYYY-MM-DD");
    }
    let date = time::Date::from_calendar_date(
        patch[..4].parse()?,
        time::Month::try_from(patch[5..7].parse::<u8>()?)?,
        patch[8..].parse()?,
    )
    .context("invalid security patch date")?;
    let _ = date;
    let compact = patch.replace('-', "");
    Ok(match backend {
        Backend::TrickyStore | Backend::TrickyStoreLegacy => [
            ("os_patch", compact[..6].into()),
            ("vendor_patch", compact.clone()),
            ("boot_patch", compact),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect(),
        Backend::TeeSimulator => ["os_patch", "vendor_patch", "boot_patch"]
            .into_iter()
            .map(|k| (k.into(), patch.into()))
            .collect(),
        Backend::OhMyKeymint => [
            "security_patch",
            "os_patchlevel",
            "vendor_patchlevel",
            "boot_patchlevel",
        ]
        .into_iter()
        .map(|k| (k.into(), patch.into()))
        .collect(),
    })
}
pub fn preview(ctx: &Context, request: &Request) -> Result<Value> {
    let _lock = crate::config_lock::acquire(ctx)?;
    let (detection, adapter) = active_adapter(&ctx.sysroot)?;
    let before = adapter.read(&ctx.sysroot)?;
    let values = proposed(detection.backend, &request.patch)?;
    let mut candidate = before.default_policy.clone();
    candidate.extend(values.clone());
    crate::policy::sanitize(&adapter.policy_schema(), candidate)?;
    let changes: Vec<_> = values
        .iter()
        .filter(|(k, v)| before.default_policy.get(*k) != Some(*v))
        .map(|(k, v)| json!({"key":k,"before":before.default_policy.get(k),"after":v}))
        .collect();
    Ok(
        json!({"backend":detection.backend,"patch":request.patch,"revision":revision(&snapshot(ctx,adapter.config_dir(),detection.backend)?)?,"changes":changes}),
    )
}
/// Render and durably back up a preview without changing live keystore files.
pub fn prepare(ctx: &Context, request: &Request) -> Result<Value> {
    let _lock = crate::config_lock::acquire(ctx)?;
    let (detection, adapter) = active_adapter(&ctx.sysroot)?;
    let before = snapshot(ctx, adapter.config_dir(), detection.backend)?;
    if revision(&before)? != request.revision {
        bail!("keystore configuration changed; preview again");
    }
    let original = adapter.read(&ctx.sysroot)?;
    let mut config = original.clone();
    config
        .default_policy
        .extend(proposed(detection.backend, &request.patch)?);
    // Validate managed fields without dropping backend extensions from the snapshot.
    crate::policy::sanitize(&adapter.policy_schema(), config.default_policy.clone())?;
    let restart = adapter
        .restart_keys()
        .iter()
        .any(|key| original.default_policy.get(*key) != config.default_policy.get(*key));
    let temporary = tempfile::tempdir()?;
    let staged = Context {
        paths: ctx.paths.clone(),
        sysroot: duck_core::Sysroot::new(temporary.path()),
    };
    restore_files(&staged, adapter.config_dir(), &before)?;
    adapter.stage_dependencies(&ctx.sysroot, &staged.sysroot)?;
    adapter.write(&staged.sysroot, &config)?;
    let after = snapshot(&staged, adapter.config_dir(), detection.backend)?;
    let token = hex::encode(Sha256::digest(serde_json::to_vec(&(&before, &after))?));
    let path = backup_path(ctx, &token)?;
    private_backup(
        &path,
        &Backup {
            backend: detection.backend,
            before,
            applied_revision: revision(&after)?,
            after: Some(after),
            restart_required: restart,
        },
    )?;
    Ok(json!({"backup":token,"restart_required":restart}))
}
fn backup_path(ctx: &Context, token: &str) -> Result<std::path::PathBuf> {
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid backup token");
    }
    Ok(ctx
        .paths
        .var_dir
        .join("tricky-store/patch-backups")
        .join(format!("{token}.json")))
}
/// Publish only the adapter-rendered files recorded during preparation.
pub fn commit_prepared(ctx: &Context, token: &str) -> Result<Value> {
    let _lock = crate::config_lock::acquire(ctx)?;
    let path = backup_path(ctx, token)?;
    let saved: Backup = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let (detection, adapter) = active_adapter(&ctx.sysroot)?;
    if detection.backend != saved.backend
        || snapshot(ctx, adapter.config_dir(), detection.backend)? != saved.before
    {
        bail!(
            "keystore configuration changed; preview again; backup: {}",
            path.display()
        );
    }
    let after = saved
        .after
        .as_ref()
        .context("backup has no prepared candidate")?;
    transaction(ctx, adapter.config_dir(), &saved.before, &path, || {
        restore_files(ctx, adapter.config_dir(), after)?;
        if snapshot(ctx, adapter.config_dir(), detection.backend)? != *after {
            bail!("keystore readback failed");
        }
        Ok(())
    })?;
    Ok(json!({"backup":token,"restart_required":saved.restart_required}))
}
pub fn apply(ctx: &Context, request: &Request) -> Result<Value> {
    let prepared = prepare(ctx, request)?;
    commit_prepared(
        ctx,
        prepared["backup"]
            .as_str()
            .context("missing prepared backup")?,
    )
}
fn transaction<T>(
    ctx: &Context,
    dir: &str,
    before: &Snapshot,
    backup_path: &std::path::Path,
    publish: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match publish() {
        Ok(value) => Ok(value),
        Err(error) => {
            if let Err(recovery) = restore_files(ctx, dir, before) {
                bail!("{error:#}; {recovery:#}; backup: {}", backup_path.display());
            }
            Err(error.context(format!(
                "original configuration restored; backup: {}",
                backup_path.display()
            )))
        }
    }
}
fn private_backup(path: &std::path::Path, backup: &Backup) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
    }
    write_string_atomic(path, &serde_json::to_string(backup)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    fs::File::open(path.parent().context("no backup directory")?)?.sync_all()?;
    Ok(())
}
pub fn restore(ctx: &Context, request: &Request) -> Result<Value> {
    let _lock = crate::config_lock::acquire(ctx)?;
    let path = backup_path(ctx, &request.backup)?;
    let backup: Backup = serde_json::from_str(&fs::read_to_string(&path)?)?;
    let (detection, adapter) = active_adapter(&ctx.sysroot)?;
    let current = snapshot(ctx, adapter.config_dir(), detection.backend)?;
    let compatible = if let Some(after) = &backup.after {
        current.iter().all(|(name, value)| {
            backup.before.get(name) == Some(value) || after.get(name) == Some(value)
        })
    } else {
        revision(&current)? == backup.applied_revision || current == backup.before
    };
    if detection.backend != backup.backend || !compatible {
        bail!(
            "keystore changed after apply; restore refused; backup: {}",
            path.display()
        );
    }
    let before_policy = adapter.read(&ctx.sysroot)?.default_policy;
    restore_files(ctx, adapter.config_dir(), &backup.before)
        .with_context(|| format!("backup: {}", path.display()))?;
    let after_policy = adapter.read(&ctx.sysroot)?.default_policy;
    let restart = adapter
        .restart_keys()
        .iter()
        .any(|key| before_policy.get(*key) != after_policy.get(*key));
    Ok(json!({"restored":true,"restart_required":restart}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prepared_restore_handles_partial_publication_and_restart_keys() {
        for (id, code, dir, original, restart) in [
            (
                "oh_my_keymint",
                1,
                "/data/misc/keystore/omk",
                "[trust]\nboot_patchlevel=\"2026-09-05\"\n",
                true,
            ),
            ("tricky_store", 246, "/data/adb/tricky_store", "", false),
        ] {
            let root = tempfile::tempdir().unwrap();
            let ctx = Context {
                paths: duck_core::AppPaths::for_root(root.path(), &root.path().join("runtime")),
                sysroot: duck_core::Sysroot::new(root.path()),
            };
            let module = ctx.sysroot.path(format!("/data/adb/modules/{id}"));
            fs::create_dir_all(&module).unwrap();
            fs::write(
                module.join("module.prop"),
                format!("id={id}\nversionCode={code}\n"),
            )
            .unwrap();
            let name = if restart { "config.toml" } else { "config.ini" };
            let file = ctx.sysroot.path(format!("{dir}/{name}"));
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(&file, original).unwrap();
            let mut request = Request {
                patch: "2026-10-05".into(),
                ..Default::default()
            };
            request.revision = preview(&ctx, &request).unwrap()["revision"]
                .as_str()
                .unwrap()
                .into();
            let prepared = prepare(&ctx, &request).unwrap();
            request.backup = prepared["backup"].as_str().unwrap().into();
            assert_eq!(fs::read_to_string(&file).unwrap(), original);
            assert_eq!(
                commit_prepared(&ctx, &request.backup).unwrap()["restart_required"],
                restart
            );
            assert_eq!(
                restore(&ctx, &request).unwrap()["restart_required"],
                restart
            );
            assert_eq!(fs::read_to_string(&file).unwrap(), original);
            assert_eq!(restore(&ctx, &request).unwrap()["restart_required"], false);
            commit_prepared(&ctx, &request.backup).unwrap();
            if restart {
                // Power loss after publishing only one of the adapter's two files.
                fs::remove_file(ctx.sysroot.path(format!("{dir}/injector.toml"))).unwrap();
            }
            restore(&ctx, &request).unwrap();
            commit_prepared(&ctx, &request.backup).unwrap();
            fs::write(&file, "# changed externally\n").unwrap();
            assert!(restore(&ctx, &request).is_err());
            assert_eq!(fs::read_to_string(file).unwrap(), "# changed externally\n");
        }
    }
    #[test]
    fn mappings_are_backend_specific() {
        let date = "2026-10-05";
        assert_eq!(
            proposed(Backend::TrickyStore, date).unwrap()["os_patch"],
            "202610"
        );
        assert_eq!(
            proposed(Backend::TrickyStoreLegacy, date).unwrap()["boot_patch"],
            "20261005"
        );
        assert_eq!(
            proposed(Backend::TeeSimulator, date).unwrap()["os_patch"],
            date
        );
        assert_eq!(
            proposed(Backend::OhMyKeymint, date).unwrap()["security_patch"],
            date
        );
        assert!(proposed(Backend::TrickyStore, "2026-02-31").is_err());
    }
    #[test]
    fn readback_failure_restores_the_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let ctx = Context {
            paths: duck_core::AppPaths::for_root(root.path(), &root.path().join("runtime")),
            sysroot: duck_core::Sysroot::new(root.path()),
        };
        let dir = "/data/adb/tricky_store";
        let path = ctx.sysroot.path(format!("{dir}/config.ini"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "original\n").unwrap();
        let before = snapshot(&ctx, dir, Backend::TrickyStore).unwrap();
        let result: Result<()> =
            transaction(&ctx, dir, &before, &root.path().join("backup.json"), || {
                fs::write(&path, vec![b'x'; 1024 * 1024 + 1])?;
                snapshot(&ctx, dir, Backend::TrickyStore)?;
                Ok(())
            });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("original configuration restored")
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "original\n");
    }
}
