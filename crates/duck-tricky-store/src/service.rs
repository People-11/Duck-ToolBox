//! Feature-level operations the CLI exposes: status aggregation and saving.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::Result;
use duck_core::{Context, Sysroot};
use duck_platform::{
    packages::{self, PackageFilter},
    root::{self, RootManager},
};
use serde::Serialize;

use crate::{
    adapters::{ConfigAdapter, for_backend},
    detect,
    error::TrickyError,
    keybox,
    model::{
        BackendDetection, ConfigData, KeyboxStatus, PackageEntry, PolicySchema, SaveRequest,
        TargetMode,
    },
    state::State,
    targets::{normalize_packages, normalize_targets},
};

#[derive(Debug, Serialize)]
pub struct StatusData {
    pub backends: Vec<BackendDetection>,
    pub active: Option<BackendDetection>,
    pub schema: Option<PolicySchema>,
    pub config: ConfigData,
    /// Set when the active backend's config exists but could not be read.
    pub config_error: Option<String>,
    pub keybox: Option<KeyboxStatus>,
    pub packages: Vec<PackageEntry>,
    pub system_apps: Vec<String>,
    pub root_manager: Option<RootManager>,
}

#[derive(Debug, Serialize)]
pub struct SaveData {
    pub config: ConfigData,
    pub backend: crate::model::Backend,
    pub target_count: usize,
    pub system_app_count: usize,
    /// A changed policy key only takes effect once the backend restarts (a reboot).
    pub restart_required: bool,
}

pub(crate) fn active_adapter(
    sysroot: &Sysroot,
) -> Result<(BackendDetection, Box<dyn ConfigAdapter>)> {
    let detection = detect::detect_active(sysroot).ok_or(TrickyError::NoBackend)?;
    let adapter = for_backend(detection.backend);
    Ok((detection, adapter))
}

pub fn status(ctx: &Context) -> Result<StatusData> {
    let state = State::load(&ctx.paths, &ctx.sysroot)?;
    let backends = detect::detect_all(&ctx.sysroot);
    let active = detect::detect_active(&ctx.sysroot);

    let (schema, config, config_error, keybox) = match &active {
        Some(detection) => {
            let adapter = for_backend(detection.backend);
            let (config, error) = match adapter.read(&ctx.sysroot) {
                Ok(config) => (config, None),
                Err(error) => (ConfigData::default(), Some(format!("{error:#}"))),
            };
            let keybox = keybox::status(&ctx.sysroot, &adapter.keybox_path(&ctx.sysroot));
            (Some(adapter.policy_schema()), config, error, Some(keybox))
        }
        None => (None, ConfigData::default(), None, None),
    };

    let packages = package_entries(&config, &state.system_apps);

    Ok(StatusData {
        backends,
        active,
        schema,
        config,
        config_error,
        keybox,
        packages,
        system_apps: state.system_apps,
        root_manager: root::detect(&ctx.sysroot),
    })
}

/// e.g. `Tricky Store (TS): 23 targets`. Skips the package manager, so it
/// stays fast enough to run from a boot script.
pub fn status_line(ctx: &Context) -> Option<String> {
    let Some(detection) = detect::detect_active(&ctx.sysroot) else {
        return Some("Keystore: no Tricky Store, TEESimulator or OhMyKeymint found".into());
    };
    let adapter = for_backend(detection.backend);
    let name = detection
        .name
        .as_deref()
        .unwrap_or(detection.module_id.as_str());
    let targets = adapter
        .read(&ctx.sysroot)
        .map(|config| config.targets.len())
        .unwrap_or(0);
    let state = if detection.active {
        ""
    } else {
        ", module disabled"
    };
    Some(format!(
        "{name} ({}): {targets} targets{state}",
        detection.identity
    ))
}

pub fn save(ctx: &Context, request: SaveRequest) -> Result<SaveData> {
    let _lock = crate::config_lock::acquire(ctx)?;
    let (detection, adapter) = active_adapter(&ctx.sysroot)?;
    if request
        .expected_backend
        .is_some_and(|expected| expected != detection.backend)
    {
        anyhow::bail!("keystore backend changed; refresh before saving");
    }
    let previous = adapter.read(&ctx.sysroot)?;
    if request
        .expected_config
        .as_ref()
        .is_some_and(|expected| expected != &previous)
    {
        anyhow::bail!("keystore configuration changed; refresh before saving");
    }
    let schema = adapter.policy_schema();

    let targets = normalize_targets(request.targets, schema.supports_app_mode);
    let targeted: BTreeSet<&str> = targets
        .iter()
        .map(|entry| entry.package_name.as_str())
        .collect();
    let mut per_app_policy = BTreeMap::new();
    if schema.supports_per_app_policy {
        let mut submitted = request.per_app_policy;
        for package in targeted {
            let empty = BTreeMap::new();
            let policy = crate::policy::update(
                &schema,
                submitted.remove(package).unwrap_or_default(),
                previous.per_app_policy.get(package).unwrap_or(&empty),
            )?;
            if !policy.is_empty() {
                per_app_policy.insert(package.to_owned(), policy);
            }
        }
    }
    let default_policy =
        crate::policy::update(&schema, request.default_policy, &previous.default_policy)?;
    let restart_required = adapter.restart_keys().iter().any(|key| {
        default_policy
            .get(*key)
            .is_some_and(|value| previous.default_policy.get(*key) != Some(value))
    });

    let mut state = State::load(&ctx.paths, &ctx.sysroot)?;
    state.system_apps = normalize_packages(request.system_apps);
    let target_count = targets.len();
    let config = ConfigData {
        targets,
        default_policy,
        per_app_policy,
    };
    let mut paths = crate::config_update::paths(ctx, adapter.as_ref());
    paths.push(State::path(&ctx.paths));
    let saved = crate::config_update::apply(&paths, || {
        adapter.write(&ctx.sysroot, &config)?;
        state.save(&ctx.paths)?;
        adapter.read(&ctx.sysroot)
    })?;

    Ok(SaveData {
        config: saved,
        backend: adapter.backend(),
        target_count,
        system_app_count: state.system_apps.len(),
        restart_required,
    })
}

/// User apps, every system app, plus targets that are no longer installed so they can
/// still be deselected.
fn package_entries(config: &ConfigData, system_apps: &[String]) -> Vec<PackageEntry> {
    let modes: BTreeMap<&str, TargetMode> = config
        .targets
        .iter()
        .map(|entry| (entry.package_name.as_str(), entry.mode))
        .collect();
    let tracked: BTreeSet<&str> = system_apps.iter().map(String::as_str).collect();
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();

    let mut push = |name: String, system: bool| {
        if seen.insert(name.clone()) {
            entries.push(PackageEntry {
                selected: modes.contains_key(name.as_str()),
                mode: modes.get(name.as_str()).copied().unwrap_or_default(),
                tracked_system: tracked.contains(name.as_str()),
                system,
                package_name: name,
            });
        }
    };

    for name in packages::list(PackageFilter::User) {
        push(name, false);
    }
    for name in packages::list(PackageFilter::System) {
        push(name, true);
    }
    for name in modes.keys() {
        push((*name).to_owned(), tracked.contains(name));
    }

    entries.sort_by(|left, right| left.package_name.cmp(&right.package_name));
    entries
}
