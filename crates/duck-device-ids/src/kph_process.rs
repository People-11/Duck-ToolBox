//! Bounded KPH process ownership; diagnostics are returned only for private reporting.
use crate::DeviceIdsError;
use anyhow::{Context, Result};
use std::{
    ffi::OsStr,
    fs::{self, File},
    io::Read,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

const MAX_DIAGNOSTIC_BYTES: u64 = 8192;

pub(crate) struct Runner {
    pub executable: PathBuf,
    pub directory: tempfile::TempDir,
    pub timeout: Duration,
}

pub(crate) struct Output {
    pub status: ExitStatus,
    pub stdout: String,
    pub stderr: String,
}

impl Runner {
    pub fn run<S: AsRef<OsStr>>(&self, args: &[S]) -> Result<Output> {
        let output_path = self.directory.path().join("stdout");
        let error_path = self.directory.path().join("stderr");
        let open = |path: &Path| -> Result<Stdio> {
            Ok(Stdio::from(
                File::options()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(path)?,
            ))
        };
        let child = Command::new(&self.executable)
            .args(args)
            .current_dir(self.directory.path())
            .env(
                "LD_LIBRARY_PATH",
                "/system/lib64:/vendor/lib64:/odm/lib64:/vendor/lib64/hw:/odm/lib64/hw",
            )
            .stdin(Stdio::null())
            .stdout(open(&output_path)?)
            .stderr(open(&error_path)?)
            .spawn()
            .context("start KPH")?;
        let mut child = Process(child);
        let deadline = Instant::now() + self.timeout;
        let status = loop {
            if let Some(status) = child.0.try_wait()? {
                break status;
            }
            self.check_output_limit()?;
            if Instant::now() >= deadline {
                return Err(DeviceIdsError::Timeout.into());
            }
            thread::sleep(Duration::from_millis(10));
        };
        self.check_output_limit()?;
        let (stdout, stderr) = self.diagnostics()?;
        Ok(Output {
            status,
            stdout,
            stderr,
        })
    }

    /// Also available after an error has killed/reaped the child. Oversized streams
    /// retain their bounded prefix so failure reports keep the original TA diagnostic.
    pub fn diagnostics(&self) -> Result<(String, String)> {
        let read = |name| -> Result<String> {
            let file = File::options()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
                .open(self.directory.path().join(name))?;
            anyhow::ensure!(file.metadata()?.is_file(), "invalid KPH diagnostic file");
            let mut bytes = vec![];
            file.take(MAX_DIAGNOSTIC_BYTES).read_to_end(&mut bytes)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        Ok((read("stdout")?, read("stderr")?))
    }

    fn check_output_limit(&self) -> Result<()> {
        for name in ["stdout", "stderr"] {
            if fs::metadata(self.directory.path().join(name))?.len() > MAX_DIAGNOSTIC_BYTES {
                anyhow::bail!("KPH diagnostic output exceeds {MAX_DIAGNOSTIC_BYTES} bytes");
            }
        }
        Ok(())
    }
}

struct Process(Child);
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(crate) fn directory(parent: &Path) -> Result<tempfile::TempDir> {
    fs::create_dir_all(parent)?;
    Ok(tempfile::Builder::new()
        .prefix("device-ids-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(parent)?)
}

pub(crate) struct Lock(File);

impl Drop for Lock {
    fn drop(&mut self) {
        // Release explicitly: concurrent process creation can briefly inherit the
        // open file description before close-on-exec closes its descriptor.
        let _ = self.0.unlock();
    }
}

pub(crate) fn lock(parent: &Path) -> Result<Lock> {
    fs::create_dir_all(parent)?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .mode(0o600)
        .open(parent.join("device-ids.lock"))?;
    match file.try_lock() {
        Ok(()) => (),
        Err(fs::TryLockError::WouldBlock) => return Err(DeviceIdsError::Busy.into()),
        Err(fs::TryLockError::Error(error)) => {
            return Err(error).context("lock device ID provisioning");
        }
    }
    Ok(Lock(file))
}
