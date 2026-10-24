use crate::{
    DeviceIdsProfile,
    backend::SessionInfo,
    plan::{Plan, Request},
};
use anyhow::{Context, Result};
use duck_core::AppPaths;
use serde::Serialize;
use std::{
    fs,
    io::Write,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

pub(crate) struct Report {
    path: PathBuf,
}

impl Report {
    pub fn new(paths: &AppPaths) -> Result<Self> {
        fs::create_dir_all(&paths.outputs_dir)?;
        let directory = tempfile::Builder::new()
            .prefix("device-ids-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&paths.outputs_dir)?
            .keep();
        Ok(Self {
            path: directory.join("device_ids_report.json"),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write(
        &self,
        profile: &DeviceIdsProfile,
        plan: &Plan,
        session: &SessionInfo,
        status: &str,
        error: Option<&anyhow::Error>,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct Snapshot<'a> {
            profile: &'a DeviceIdsProfile,
            #[serde(flatten)]
            plan: &'a Plan,
            #[serde(flatten)]
            session: &'a SessionInfo,
            status: &'a str,
            dry_run: bool,
            command_hex: Option<String>,
            error: Option<String>,
        }
        let snapshot = Snapshot {
            profile,
            plan,
            session,
            status,
            dry_run: profile.dry_run,
            command_hex: match &plan.request {
                Request::Qseecom { command } => Some(hex::encode(command)),
                _ => None,
            },
            error: error.map(|e| format!("{e:#}")),
        };
        let mut file = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        file.write_all(&serde_json::to_vec_pretty(&snapshot)?)?;
        file.as_file().sync_all()?;
        file.persist(&self.path)
            .with_context(|| format!("publish {}", self.path.display()))?;
        Ok(())
    }
}
