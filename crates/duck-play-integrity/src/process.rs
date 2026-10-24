//! Bounded process boundary for the independent engine's public protocol.
use anyhow::{Context as _, Result, bail};
use duck_core::Context;
use serde_json::Value;
use std::{
    io::{Read, Seek, Write},
    process::{Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub struct EngineError(pub Value);
impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            self.0["message"].as_str().unwrap_or("engine failed")
        )
    }
}
impl std::error::Error for EngineError {}

const OUTPUT_LIMIT: u64 = 2 * 1024 * 1024;

#[cfg(unix)]
#[allow(unsafe_code)]
fn kill_owned_group(id: u32) {
    let Ok(id) = i32::try_from(id) else {
        return;
    };
    if id <= 0 {
        return;
    }
    // SAFETY: kill takes no pointers. Only a positive, owned process-group ID
    // reaches this call; negation never signals this process's group or all PIDs.
    unsafe {
        libc::kill(-id, libc::SIGKILL);
    }
}

fn exchange(
    mut command: Command,
    input: &[u8],
    timeout: Duration,
) -> Result<(ExitStatus, Vec<u8>, Vec<u8>)> {
    if input.len() > 128 * 1024 {
        bail!("engine input exceeds 128 KiB");
    }
    // Anonymous files keep stdin writes and output collection independent of pipe
    // capacity and EOF held by descendants. No request data remains at a named path.
    let mut request = tempfile::tempfile()?;
    request.write_all(input)?;
    request.rewind()?;
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command
        .stdin(Stdio::from(request))
        .stdout(Stdio::from(stdout.try_clone()?))
        .stderr(Stdio::from(stderr.try_clone()?))
        .spawn()?;
    let deadline = Instant::now() + timeout;
    let waited = (|| -> Result<ExitStatus> {
        loop {
            if stdout.metadata()?.len() > OUTPUT_LIMIT || stderr.metadata()?.len() > OUTPUT_LIMIT {
                bail!("engine output exceeds 2 MiB");
            }
            if let Some(status) = child.try_wait()? {
                return Ok(status);
            }
            if Instant::now() >= deadline {
                bail!(
                    "engine request timed out after {} seconds",
                    timeout.as_secs()
                );
            }
            thread::sleep(Duration::from_millis(10));
        }
    })();
    // Background jobs explicitly create their own groups. Reap only descendants
    // belonging to this exchange, including when the engine exits before them.
    #[cfg(unix)]
    kill_owned_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
    let status = waited?;
    let capture = |file: &mut std::fs::File| -> Result<Vec<u8>> {
        file.rewind()?;
        let mut bytes = Vec::new();
        file.take(OUTPUT_LIMIT + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > OUTPUT_LIMIT {
            bail!("engine output exceeds 2 MiB");
        }
        Ok(bytes)
    };
    Ok((status, capture(&mut stdout)?, capture(&mut stderr)?))
}
pub fn request(ctx: &Context, operation: &str, input: &Value) -> Result<Value> {
    let module = ctx.paths.root.join("play-integrity");
    let executable = module.join("pif-engine");
    let mut command = Command::new(&executable);
    command
        .args(["request", operation])
        .env("PIF_ENGINE_MODULE", &module)
        .env(
            "PIF_ENGINE_DATA",
            ctx.sysroot.path("/data/adb/play-integrity"),
        )
        .env("PIF_ENGINE_SYSROOT", ctx.sysroot.root());
    let (status, stdout, stderr) = exchange(
        command,
        &serde_json::to_vec(input)?,
        Duration::from_secs(15),
    )
    .with_context(|| format!("engine {}", executable.display()))?;
    let response: Value = serde_json::from_slice(&stdout).with_context(|| {
        format!(
            "invalid engine response: {}",
            String::from_utf8_lossy(&stderr)
        )
    })?;
    if response["api"] != 1 {
        bail!("unsupported engine protocol version");
    }
    if response["ok"] != true {
        return Err(EngineError(response["error"].clone()).into());
    }
    if !status.success() {
        bail!("engine exited with {status}");
    }
    response
        .get("data")
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("engine response is missing data"))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    #[test]
    fn unread_large_input_cannot_block_the_deadline() {
        let mut command = Command::new("sh");
        command.args(["-c", "exec sleep 30"]);
        let started = Instant::now();
        let error =
            exchange(command, &vec![b'x'; 128 * 1024], Duration::from_millis(100)).unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn child_output_is_capped_before_protocol_parsing() {
        let mut command = Command::new("sh");
        command.args(["-c", "dd if=/dev/zero bs=1048576 count=3 2>/dev/null"]);
        assert!(
            exchange(command, b"", Duration::from_secs(2))
                .unwrap_err()
                .to_string()
                .contains("exceeds 2 MiB")
        );
    }

    #[test]
    fn exchange_reaps_owned_children_and_preserves_detached_jobs() {
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                for pid in std::fs::read_to_string(&self.0).unwrap_or_default().lines() {
                    if let Ok(pid) = pid.parse::<u32>() {
                        kill_owned_group(pid);
                    }
                }
            }
        }
        for mode in ["timeout", "normal", "detached"] {
            let root = tempfile::tempdir().unwrap();
            let heartbeat = root.path().join("heartbeat");
            let groups = root.path().join("groups");
            let _cleanup = Cleanup(groups.clone());
            let mut command = Command::new("python3");
            // Keep cleanup safe when running this regression against the old adapter.
            use std::os::unix::process::CommandExt;
            command.process_group(0);
            command
                .args([
                    "-c",
                    r#"
import os,signal,time
mode=os.environ['MODE'];heartbeat=os.environ['HEARTBEAT'];groups=os.environ['GROUPS_FILE']
with open(groups,'a') as f:f.write(str(os.getpid())+'\n')
if os.fork()==0:
 if mode=='detached':
  os.setpgid(0,0)
  with open(groups,'a') as f:f.write(str(os.getpid())+'\n')
 signal.signal(signal.SIGTERM,signal.SIG_IGN)
 while True:
  with open(heartbeat,'a') as f:f.write('x')
  time.sleep(.01)
else:
 while not os.path.exists(heartbeat):time.sleep(.01)
 if mode=='timeout':time.sleep(30)
 print('done',flush=True)
"#,
                ])
                .env("MODE", mode)
                .env("HEARTBEAT", &heartbeat)
                .env("GROUPS_FILE", groups);
            let result = exchange(command, b"", Duration::from_millis(500));
            if mode == "timeout" {
                assert!(result.unwrap_err().to_string().contains("timed out"));
            } else {
                let (status, output, _) = result.unwrap();
                assert!(status.success());
                assert_eq!(output, b"done\n");
            }
            let before = std::fs::metadata(&heartbeat).unwrap().len();
            thread::sleep(Duration::from_millis(150));
            let after = std::fs::metadata(&heartbeat).unwrap().len();
            assert_eq!(after > before, mode == "detached", "{mode}");
        }
    }
}
