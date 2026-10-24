//! Private request/reply protocol for isolated calls into the vendor library.
use crate::{DeviceIdsError, plan::DataItem};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub(crate) const MAX_REQUEST: usize = 64 * 1024;
const MAX_REPLY: usize = 16 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Request {
    pub library: PathBuf,
    pub call: Call,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case")]
pub(crate) enum Call {
    Version,
    Import { item: DataItem },
    Verify { uuid: String, tag: String },
}

impl Call {
    pub fn validate(&self) -> Result<()> {
        let (uuid, tag) = match self {
            Self::Version => return Ok(()),
            Self::Import { item } => {
                ensure!(
                    item.permission == 42 && item.flags == 0,
                    "unsupported TrustKernel context/storage parameters"
                );
                ensure!(
                    !item.payload.is_empty()
                        && item.payload.len() <= 16 * 1024
                        && item.payload.last() == Some(&0)
                        && !item.payload[..item.payload.len() - 1].contains(&0),
                    "invalid TrustKernel payload length or NUL terminator"
                );
                (item.uuid.as_str(), item.tag.as_str())
            }
            Self::Verify { uuid, tag } => (uuid.as_str(), tag.as_str()),
        };
        ensure!(
            matches!(
                (uuid, tag),
                ("b46325e6-5c90-8252-2ead-a8e32e5180d6", "rkp_flag")
                    | ("2ea702fa-17bc-4752-b3ad-b2871a772347", "attestation_ids")
            ),
            "unsupported TrustKernel target"
        );
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub(crate) enum Reply {
    Returned {
        library: PathBuf,
        status: i32,
        version: Option<String>,
    },
    Failed {
        message: String,
    },
}

pub(crate) fn execute(directory: &Path) -> Result<()> {
    let result = (|| -> Result<Reply> {
        let request: Request =
            serde_json::from_slice(&read(&directory.join("request.json"), MAX_REQUEST)?)?;
        request.call.validate()?;
        ensure!(
            cfg!(all(target_os = "android", target_arch = "aarch64")),
            "Android arm64 is required"
        );
        crate::trustkernel_native::call(&request.library, &request.call)
    })();
    let reply = result.unwrap_or_else(|error| Reply::Failed {
        message: format!("{error:#}"),
    });
    write(&directory.join("reply.json"), &reply)
}

pub(crate) fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("publish {}", path.display()))?;
    Ok(())
}

pub(crate) fn read(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= limit as u64,
        "invalid worker exchange file"
    );
    let mut bytes = vec![];
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "worker exchange file exceeds size limit"
    );
    Ok(bytes)
}

pub(crate) fn invoke(
    runner: &crate::kph_process::Runner,
    request: &Request,
    session: &mut crate::backend::SessionInfo,
    stage: &'static str,
    tag: Option<&str>,
) -> Result<Reply> {
    request.call.validate()?;
    let path = runner.directory.path();
    let reply_path = path.join("reply.json");
    if reply_path.try_exists()? {
        fs::remove_file(&reply_path)?;
    }
    write(&path.join("request.json"), request)?;
    let args: [std::ffi::OsString; 4] = [
        "device-ids".into(),
        "kph-call".into(),
        "--exchange".into(),
        path.as_os_str().into(),
    ];
    let output = match runner.run(&args) {
        Ok(output) => output,
        Err(error) => {
            let (stdout, stderr) = runner.diagnostics().unwrap_or_default();
            session.diagnostics.push(crate::backend::Diagnostic {
                stage,
                tag: tag.map(str::to_owned),
                status: format!("{error:#}"),
                stdout,
                stderr,
            });
            return Err(error);
        }
    };
    session.diagnostics.push(crate::backend::Diagnostic {
        stage,
        tag: tag.map(str::to_owned),
        status: output.status.to_string(),
        stdout: output.stdout,
        stderr: output.stderr,
    });
    if !output.status.success() || !reply_path.is_file() {
        return Err(DeviceIdsError::Transport(format!(
            "worker exited with {}; vendor ABI may be incompatible",
            output.status
        ))
        .into());
    }
    let reply: Reply =
        (|| -> Result<Reply> { Ok(serde_json::from_slice(&read(&reply_path, MAX_REPLY)?)?) })()
            .map_err(|error| {
                DeviceIdsError::Transport(format!("invalid worker reply: {error:#}"))
            })?;
    match &reply {
        Reply::Returned { library, .. } if library != &request.library => Err(
            DeviceIdsError::Transport("worker library disagrees with the requested library".into())
                .into(),
        ),
        Reply::Failed { message } => Err(DeviceIdsError::Transport(message.clone()).into()),
        Reply::Returned { status, .. } => {
            session
                .diagnostics
                .last_mut()
                .unwrap()
                .status
                .push_str(&format!("; KPH status {status} (0x{:08x})", *status as u32));
            Ok(reply)
        }
    }
}
