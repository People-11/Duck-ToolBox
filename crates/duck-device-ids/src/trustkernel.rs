//! TrustKernel provisioning owns sequencing and failure state; vendor calls run in children.
use std::{path::Path, time::Duration};

use anyhow::Result;

use crate::{
    DeviceIdsError,
    backend::{SessionInfo, WriteOutcome},
    kph_process::{self, Runner},
    plan::DataItem,
    trustkernel_worker::{self, Call, Reply, Request},
};

pub(crate) fn provision(
    ctx: &duck_core::Context,
    items: &[DataItem],
    session: &mut SessionInfo,
) -> Result<()> {
    if !ctx.sysroot.path("/dev/tkcoredrv").exists() {
        return Err(DeviceIdsError::Unsupported("TrustKernel /dev/tkcoredrv is required").into());
    }
    let library = [
        "/vendor/lib64/libkphproxy.so",
        "/odm/lib64/libkphproxy.so",
        "/vendor/lib64/hw/libkphproxy.so",
        "/odm/lib64/hw/libkphproxy.so",
        "/system/lib64/libkphproxy.so",
    ]
    .iter()
    .map(|path| ctx.sysroot.path(path))
    .find(|path| path.is_file())
    .ok_or(DeviceIdsError::Unsupported(
        "TrustKernel libkphproxy.so is required",
    ))?;
    let _lock = kph_process::lock(&ctx.paths.var_dir)?;
    let runner = Runner {
        executable: std::env::current_exe()?,
        directory: kph_process::directory(&ctx.paths.tmp_dir)?,
        timeout: Duration::from_secs(30),
    };
    execute(&runner, &library, items, session)
}

pub(crate) fn execute(
    runner: &Runner,
    library: &Path,
    items: &[DataItem],
    session: &mut SessionInfo,
) -> Result<()> {
    let invoke = |call, session: &mut SessionInfo, stage, tag| {
        trustkernel_worker::invoke(
            runner,
            &Request {
                library: library.to_owned(),
                call,
            },
            session,
            stage,
            tag,
        )
    };
    // Version discovery is optional; import resolves the required UUID exports before writing.
    if let Reply::Returned { version, .. } = invoke(Call::Version, session, "version", None)? {
        session.ta_version = version;
    }
    session.loaded_library = Some(library.display().to_string());
    for item in items {
        session.writes.push(WriteOutcome {
            tag: item.tag.clone(),
            import_succeeded: false,
            exists: None,
        });
        let reply = invoke(
            Call::Import { item: item.clone() },
            session,
            "import",
            Some(&item.tag),
        )?;
        check(reply, "import", &item.tag)?;
        session.writes.last_mut().unwrap().import_succeeded = true;
    }
    for (index, item) in items.iter().enumerate() {
        let reply = invoke(
            Call::Verify {
                uuid: item.uuid.clone(),
                tag: item.tag.clone(),
            },
            session,
            "verify",
            Some(&item.tag),
        )?;
        // A transport failure is indeterminate; a returned status says whether existence was confirmed.
        if let Reply::Returned { status, .. } = &reply {
            session.writes[index].exists = Some(*status == 0);
        }
        check(reply, "verify", &item.tag)?;
    }
    Ok(())
}

fn check(reply: Reply, stage: &'static str, tag: &str) -> Result<()> {
    match reply {
        Reply::Returned { status: 0, .. } => Ok(()),
        Reply::Returned { status, .. } => Err(DeviceIdsError::Kph {
            stage,
            tag: tag.to_owned(),
            status: format!("{status} (0x{:08x})", status as u32),
        }
        .into()),
        Reply::Failed { message } => Err(DeviceIdsError::Transport(message).into()),
    }
}
