use std::collections::BTreeMap;

use anyhow::Result;
use duck_core::Sysroot;
use duck_platform::soc::{self, State};
use serde::Serialize;

use crate::DeviceIdsError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Qseecom,
    Trustkernel,
}

pub(crate) fn select(properties: &BTreeMap<String, String>, root: &Sysroot) -> Result<Backend> {
    match soc::detect(properties, root).state {
        State::MediaTek => Ok(Backend::Trustkernel),
        State::Qualcomm | State::Unknown => Ok(Backend::Qseecom),
        State::Conflicting => Err(DeviceIdsError::Unsupported("conflicting SoC evidence").into()),
        State::Other => Err(DeviceIdsError::Unsupported("unsupported SoC vendor").into()),
    }
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct SessionInfo {
    pub loaded_library: Option<String>,
    pub executable: Option<String>,
    pub ta_api_version: Option<String>,
    pub ta_version: Option<String>,
    pub response_hex: Option<String>,
    pub writes: Vec<WriteOutcome>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Diagnostic {
    pub stage: &'static str,
    pub tag: Option<String>,
    pub status: String,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct WriteOutcome {
    pub tag: String,
    pub import_succeeded: bool,
    /// `verify_data` confirms existence, not equality with the submitted payload.
    pub exists: Option<bool>,
}
