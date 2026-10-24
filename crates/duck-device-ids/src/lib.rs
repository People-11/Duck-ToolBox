//! Provisions attestation device IDs through Qualcomm QSEECom or MediaTek TrustKernel.
//! Each adapter owns its transport; dry runs record the exact prepared request bytes.
//!
//! The tag values are the KeyMint `ATTESTATION_ID_*` tags from AOSP
//! `hardware/interfaces/security/keymint/aidl/android/hardware/security/keymint/Tag.aidl`.

use anyhow::Result;
use duck_platform::props;
use serde::{Deserialize, Serialize};

mod backend;
mod cli;
mod command;
mod defaults;
mod error;
mod kph_process;
mod plan;
#[cfg(test)]
mod plan_tests;
#[allow(unsafe_code)]
mod qseecom;
mod report;
mod spec;
mod trustkernel;
#[allow(unsafe_code)]
mod trustkernel_native;
#[cfg(test)]
mod trustkernel_native_tests;
#[cfg(test)]
mod trustkernel_tests;
mod trustkernel_worker;

pub use backend::Backend;
pub use cli::{Command, FEATURE, run};
pub use defaults::Defaults;
pub use error::DeviceIdsError;

const DEFAULT_TA_NAME: &str = "keymaster64";
const DEFAULT_TA_PATH: &str = "/vendor/firmware_mnt/image";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIdsProfile {
    #[serde(default)]
    pub brand: String,
    #[serde(default)]
    pub device: String,
    #[serde(default)]
    pub product: String,
    #[serde(default)]
    pub serial: String,
    #[serde(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub imei: String,
    #[serde(default)]
    pub imei2: String,
    #[serde(default)]
    pub meid: String,
    #[serde(default)]
    pub meid2: String,
    #[serde(default = "default_ta_name")]
    pub ta_name: String,
    #[serde(default = "default_ta_path")]
    pub ta_path: String,
    #[serde(default)]
    pub dry_run: bool,
}

impl Default for DeviceIdsProfile {
    fn default() -> Self {
        Self {
            brand: String::new(),
            device: String::new(),
            product: String::new(),
            serial: String::new(),
            manufacturer: String::new(),
            model: String::new(),
            imei: String::new(),
            imei2: String::new(),
            meid: String::new(),
            meid2: String::new(),
            ta_name: default_ta_name(),
            ta_path: default_ta_path(),
            dry_run: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProvisionedId {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceIdsProvisionResult {
    pub backend: Backend,
    pub verification: Verification,
    pub executable: Option<String>,
    pub omitted_ids: Vec<String>,
    pub count: usize,
    pub ids: Vec<ProvisionedId>,
    pub dry_run: bool,
    pub ta_name: Option<String>,
    pub ta_path: Option<String>,
    pub loaded_library: Option<String>,
    pub ta_api_version: Option<String>,
    pub ta_version: Option<String>,
    pub report_path: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verification {
    NotRun,
    TaAcknowledgement,
    ExistenceOnly,
}

fn default_ta_name() -> String {
    DEFAULT_TA_NAME.into()
}

fn default_ta_path() -> String {
    DEFAULT_TA_PATH.into()
}

/// Reads the current identifiers from one property snapshot.
pub fn detect_defaults() -> DeviceIdsProfile {
    defaults::detect(&props::all(), &duck_core::Sysroot::from_env()).profile
}

pub fn provision(
    ctx: &duck_core::Context,
    profile: DeviceIdsProfile,
) -> Result<DeviceIdsProvisionResult, duck_core::Failure> {
    let properties = props::all();
    let backend = backend::select(&properties, &ctx.sysroot)?;
    let detected = defaults::profile(&properties, Some(backend));
    let resolved = merge_missing(profile, detected);
    let plan = plan::Plan::build(&resolved, backend, &properties)?;
    let report = report::Report::new(&ctx.paths)?;
    let mut session = backend::SessionInfo::default();
    report.write(&resolved, &plan, &session, "prepared", None)?;
    let execution = if resolved.dry_run {
        Ok(())
    } else if !cfg!(target_os = "android") {
        Err(DeviceIdsError::Unsupported("live writes require Android").into())
    } else {
        match &plan.request {
            plan::Request::Qseecom { command } => {
                qseecom::provision(&resolved.ta_path, &resolved.ta_name, command).map(|info| {
                    session = info;
                })
            }
            plan::Request::Trustkernel { items } => {
                trustkernel::provision(ctx, items, &mut session)
            }
        }
    };
    let status = if execution.is_err() {
        "failed"
    } else if resolved.dry_run {
        "dry_run"
    } else {
        "completed"
    };
    let details = || serde_json::json!({ "report_path": report.path(), "backend": backend, "writes": session.writes });
    let saved = report.write(&resolved, &plan, &session, status, execution.as_ref().err());
    if let Err(error) = execution {
        let error = match saved {
            Ok(()) => error,
            Err(save) => error.context(format!("report update also failed: {save:#}")),
        };
        return Err(duck_core::Failure::with_details(error, details()));
    }
    saved.map_err(|error| duck_core::Failure::with_details(error, details()))?;
    Ok(DeviceIdsProvisionResult {
        backend,
        verification: if resolved.dry_run {
            Verification::NotRun
        } else if backend == Backend::Trustkernel {
            Verification::ExistenceOnly
        } else {
            Verification::TaAcknowledgement
        },
        count: plan.ids.len(),
        ids: plan.ids,
        omitted_ids: plan.omitted_ids,
        dry_run: resolved.dry_run,
        ta_name: (backend == Backend::Qseecom).then_some(resolved.ta_name),
        ta_path: (backend == Backend::Qseecom).then_some(resolved.ta_path),
        loaded_library: session.loaded_library,
        executable: session.executable,
        ta_api_version: session.ta_api_version,
        ta_version: session.ta_version,
        report_path: report.path().display().to_string(),
    })
}

fn merge_missing(mut profile: DeviceIdsProfile, detected: DeviceIdsProfile) -> DeviceIdsProfile {
    for (target, fallback) in [
        (&mut profile.brand, detected.brand),
        (&mut profile.device, detected.device),
        (&mut profile.product, detected.product),
        (&mut profile.serial, detected.serial),
        (&mut profile.manufacturer, detected.manufacturer),
        (&mut profile.model, detected.model),
        (&mut profile.imei, detected.imei),
        (&mut profile.imei2, detected.imei2),
        (&mut profile.meid, detected.meid),
        (&mut profile.meid2, detected.meid2),
        (&mut profile.ta_name, detected.ta_name),
        (&mut profile.ta_path, detected.ta_path),
    ] {
        if target.trim().is_empty() {
            *target = fallback;
        }
    }
    profile
}

#[cfg(test)]
mod tests {
    use super::detect_defaults;

    #[test]
    fn detect_defaults_keeps_ta_defaults() {
        let detected = detect_defaults();

        assert_eq!(detected.ta_name, "keymaster64");
        assert_eq!(detected.ta_path, "/vendor/firmware_mnt/image");
    }
}
