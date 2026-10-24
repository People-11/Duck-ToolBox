//! TrustKernel named-data provisioning through the device's KPH CLI.
use crate::{
    DeviceIdsError,
    backend::{Diagnostic, SessionInfo, WriteOutcome},
    kph_artifact,
    kph_process::{self, Runner},
    plan::DataItem,
};
use anyhow::Result;
use duck_core::Context;
use std::{ffi::OsString, fs, io::Write, os::unix::fs::OpenOptionsExt, time::Duration};

pub(crate) fn provision(
    ctx: &Context,
    items: &[DataItem],
    session: &mut SessionInfo,
) -> Result<()> {
    if !ctx.sysroot.path("/dev/tkcoredrv").exists() {
        return Err(DeviceIdsError::Unsupported("TrustKernel /dev/tkcoredrv is missing").into());
    }
    for library in ["libkphproxy.so", "libkphhelper.so"] {
        if ![
            "/system/lib64",
            "/vendor/lib64",
            "/odm/lib64",
            "/vendor/lib64/hw",
            "/odm/lib64/hw",
        ]
        .iter()
        .any(|directory| ctx.sysroot.path(format!("{directory}/{library}")).is_file())
        {
            return Err(DeviceIdsError::Unsupported(
                "KPH requires libkphproxy.so and libkphhelper.so on the device",
            )
            .into());
        }
    }
    let _lock = kph_process::lock(&ctx.paths.var_dir)?;
    let executable = kph_artifact::select(ctx)?;
    let directory = kph_process::directory(&ctx.paths.tmp_dir)?;
    let snapshot = kph_artifact::snapshot(&executable, directory.path())?;
    session.executable = Some(executable.source.display().to_string());
    session.executable_sha256 = Some(executable.digest);
    execute(
        &Runner {
            executable: snapshot,
            directory,
            timeout: Duration::from_secs(30),
        },
        items,
        session,
    )
}

pub(crate) fn execute(
    runner: &Runner,
    items: &[DataItem],
    session: &mut SessionInfo,
) -> Result<()> {
    let version = runner.run(&["-c", "ta_version"])?;
    record(session, "version", None, &version);
    if version.status.success() {
        let version = version
            .stdout
            .trim()
            .strip_prefix("KPH TA VERSION=")
            .unwrap_or(version.stdout.trim());
        if !version.is_empty() {
            session.ta_version = Some(version.to_owned());
        }
    }
    for item in items {
        let path = runner.directory.path().join(format!("{}.dat", item.tag));
        let mut payload = fs::File::options()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)?;
        payload.write_all(&item.payload)?;
        drop(payload);
        session.writes.push(WriteOutcome {
            tag: item.tag.into(),
            import_succeeded: false,
            exists: None,
        });
        let args: [OsString; 12] = [
            "-c".into(),
            "import_data".into(),
            "-u".into(),
            item.uuid.into(),
            "-g".into(),
            item.tag.into(),
            "-i".into(),
            path.into_os_string(),
            "-p".into(),
            item.permission.to_string().into(),
            "-f".into(),
            item.flags.to_string().into(),
        ];
        let output = runner.run(&args)?;
        record(session, "import", Some(item.tag), &output);
        if !output.status.success() {
            return Err(DeviceIdsError::Kph {
                stage: "import",
                tag: item.tag.into(),
                status: output.status.to_string(),
            }
            .into());
        }
        session.writes.last_mut().unwrap().import_succeeded = true;
    }
    for (index, item) in items.iter().enumerate() {
        let output = runner.run(&["-c", "verify_data", "-u", item.uuid, "-g", item.tag])?;
        record(session, "verify existence", Some(item.tag), &output);
        session.writes[index].exists = Some(output.status.success());
        if !output.status.success() {
            return Err(DeviceIdsError::Kph {
                stage: "verify existence",
                tag: item.tag.into(),
                status: output.status.to_string(),
            }
            .into());
        }
    }
    Ok(())
}

fn record(
    session: &mut SessionInfo,
    stage: &'static str,
    tag: Option<&str>,
    output: &kph_process::Output,
) {
    session.diagnostics.push(Diagnostic {
        stage,
        tag: tag.map(str::to_owned),
        status: output.status.to_string(),
        stdout: output.stdout.clone(),
        stderr: output.stderr.clone(),
    });
}
