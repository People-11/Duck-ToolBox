//! Auto-target: keeps the target list in step with the installed apps.
//!
//! Each run adds user apps installed since the previous run (every user app on the first
//! run) and the necessary system apps, and removes unnecessary apps: Tricky Addon's list of
//! root managers and tools, plus Xposed modules. The rest of the config is left alone.

use std::collections::BTreeSet;

use anyhow::Result;
use duck_core::Context;
use duck_platform::packages::{self, PackageFilter};
use serde::Serialize;

use crate::{
    exclude,
    model::{ConfigData, TargetEntry, TargetMode},
    service::active_adapter,
    state::State,
    targets::normalize_packages,
    xposed,
};

#[derive(Debug, Serialize)]
pub struct AutoApplyData {
    pub added: Vec<String>,
    pub removed: Vec<String>,
    pub target_count: usize,
}

/// With `refresh`, the unnecessary-apps list is fetched again and the list is reconciled
/// even when no app was installed or removed.
pub async fn apply(ctx: &Context, refresh: bool) -> Result<AutoApplyData> {
    let (_, adapter) = active_adapter(&ctx.sysroot)?;
    let mut config = adapter.read(&ctx.sysroot)?;
    let mut state = State::load(&ctx.paths, &ctx.sysroot)?;
    let user_apps = normalize_packages(packages::list(PackageFilter::User));
    let unchanged = state.auto.baseline_initialized && state.auto.known_user_apps == user_apps;
    // An empty list means `pm` is not up yet; saving it would make every app look new later.
    if user_apps.is_empty() || (unchanged && !refresh) {
        return Ok(AutoApplyData {
            added: Vec::new(),
            removed: Vec::new(),
            target_count: config.targets.len(),
        });
    }

    let (unnecessary, _) = exclude::load(ctx, refresh).await?;
    let mut excluded: BTreeSet<String> = unnecessary.into_iter().collect();
    excluded.extend(xposed::xposed_modules(ctx)?);

    let known: BTreeSet<&str> = if state.auto.baseline_initialized {
        state
            .auto
            .known_user_apps
            .iter()
            .map(String::as_str)
            .collect()
    } else {
        BTreeSet::new()
    };
    let installed: BTreeSet<String> = packages::list(PackageFilter::System)
        .into_iter()
        .chain(user_apps.iter().cloned())
        .collect();
    let wanted: Vec<String> = user_apps
        .iter()
        .filter(|app| !known.contains(app.as_str()))
        .chain(
            state
                .system_apps
                .iter()
                .filter(|app| installed.contains(*app)),
        )
        .cloned()
        .collect();

    let (added, removed) = update_targets(&mut config, &wanted, &excluded);
    if !added.is_empty() || !removed.is_empty() {
        adapter.write(&ctx.sysroot, &config)?;
    }

    state.auto.known_user_apps = user_apps;
    state.auto.baseline_initialized = true;
    state.save(&ctx.paths)?;

    Ok(AutoApplyData {
        added,
        removed,
        target_count: config.targets.len(),
    })
}

/// Drops excluded targets (and their per-app policy), then appends every wanted app that
/// is neither excluded nor already targeted. Returns `(added, removed)`.
fn update_targets(
    config: &mut ConfigData,
    wanted: &[String],
    excluded: &BTreeSet<String>,
) -> (Vec<String>, Vec<String>) {
    // TEESimulator targets may carry a `@user` suffix.
    let package = |entry: &str| entry.split('@').next().unwrap_or(entry).to_owned();

    let mut removed = Vec::new();
    config.targets.retain(|entry| {
        let keep = !excluded.contains(&package(&entry.package_name));
        if !keep {
            removed.push(entry.package_name.clone());
        }
        keep
    });
    config
        .per_app_policy
        .retain(|name, _| !excluded.contains(&package(name)));

    let present: BTreeSet<String> = config
        .targets
        .iter()
        .map(|entry| package(&entry.package_name))
        .collect();
    let added: Vec<String> = wanted
        .iter()
        .filter(|app| !excluded.contains(*app) && !present.contains(*app))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    config
        .targets
        .extend(added.iter().map(|package_name| TargetEntry {
            package_name: package_name.clone(),
            mode: TargetMode::Auto,
        }));
    (added, removed)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::update_targets;
    use crate::model::{ConfigData, TargetEntry, TargetMode};

    fn target(name: &str, mode: TargetMode) -> TargetEntry {
        TargetEntry {
            package_name: name.into(),
            mode,
        }
    }

    #[test]
    fn adds_wanted_apps_and_drops_unnecessary_ones() {
        let mut config = ConfigData {
            targets: vec![
                target("com.kept", TargetMode::Generate),
                target("com.topjohnwu.magisk", TargetMode::Auto),
                target("org.lsposed.module@10", TargetMode::Auto),
            ],
            per_app_policy: [("com.topjohnwu.magisk".to_owned(), Default::default())].into(),
            ..ConfigData::default()
        };
        let excluded: BTreeSet<String> = ["com.topjohnwu.magisk", "org.lsposed.module"]
            .map(str::to_owned)
            .into();
        let wanted = ["com.new", "com.kept", "com.topjohnwu.magisk", "com.new"].map(str::to_owned);

        let (added, removed) = update_targets(&mut config, &wanted, &excluded);

        assert_eq!(added, ["com.new"]);
        assert_eq!(removed, ["com.topjohnwu.magisk", "org.lsposed.module@10"]);
        assert_eq!(
            config.targets,
            [
                target("com.kept", TargetMode::Generate),
                target("com.new", TargetMode::Auto)
            ]
        );
        assert!(config.per_app_policy.is_empty());
    }
}
