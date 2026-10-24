// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: bounded local platform commands with isolated process groups.
use anyhow::{Result, bail};
use std::{
    io::{Read, Seek, SeekFrom},
    process::{Child, Command, ExitStatus, Output, Stdio},
    time::{Duration, Instant},
};

// Each local operation owns a separate group, including descendants that ignore SIGTERM.
pub struct Process {
    child: Child,
    stopped: bool,
}
impl Process {
    pub fn spawn(command: &mut Command) -> Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        Ok(Self {
            child: command.spawn()?,
            stopped: false,
        })
    }
    pub fn try_wait(&mut self) -> std::io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }
    pub fn terminate(&mut self) {
        if self.stopped {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.stopped = true;
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn run(command: &mut Command, timeout: Duration) -> Result<Output> {
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?);
    let mut child = Process::spawn(command)?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        if stdout.metadata()?.len() > 256 * 1024
            || stderr.metadata()?.len() > 256 * 1024
            || Instant::now() >= deadline
        {
            bail!("platform command exceeded its time or output limit");
        }
        if let Some(status) = child.try_wait()? {
            break status;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    child.terminate();
    let read = |file: &mut std::fs::File| -> Result<Vec<u8>> {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.take(256 * 1024 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > 256 * 1024 {
            bail!("platform command output too large");
        }
        Ok(bytes)
    };
    Ok(Output {
        status,
        stdout: read(&mut stdout)?,
        stderr: read(&mut stderr)?,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn a_blocked_platform_tool_does_not_block_shutdown_indefinitely() {
        let start = Instant::now();
        assert!(
            run(
                Command::new("sh").args(["-c", "sleep 30"]),
                Duration::from_millis(100)
            )
            .is_err()
        );
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
