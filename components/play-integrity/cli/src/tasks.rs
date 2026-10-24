// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: isolated fingerprint jobs, bounded lifetime and cancellation.
use crate::{Engine, command::Process, config, files};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

fn task_dir(engine: &Engine, id: &str) -> Result<std::path::PathBuf> {
    if id.is_empty() || id.len() > 64 || !id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-')
    {
        bail!("invalid task id");
    }
    Ok(engine.data.join("jobs").join(id))
}
pub fn start(engine: &Engine, input: &Value) -> Result<Value> {
    if let Some(value) = input.get("timeout_seconds")
        && !value
            .as_u64()
            .is_some_and(|seconds| (1..=120).contains(&seconds))
    {
        bail!("timeout_seconds must be 1..120");
    }
    let _lock = files::Lock::acquire(&engine.data)?;
    if crate::power::disabled(engine) {
        bail!("engine disabled; generation is stopped");
    }
    let active = files::json_file(&engine.data.join("active-job.json"))?;
    if let Some(id) = active["id"].as_str()
        && status(engine, id)?["state"] == "running"
    {
        bail!("fingerprint generation already running");
    }
    let id = format!(
        "{}-{}",
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos(),
        std::process::id()
    );
    let dir = task_dir(engine, &id)?;
    files::private_dir(&dir)?;
    files::write_json(&dir.join("request.json"), input)?;
    files::write_json(
        &dir.join("state.json"),
        &json!({"id":id,"state":"running","pid":null,"created":SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs(),"result":null,"error":null}),
    )?;
    // Publish the cancellable job identity before starting its worker.
    files::write_json(
        &engine.data.join("active-job.json"),
        &json!({"id":id,"pid":null}),
    )?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["worker", &id])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn()?;
    let pid = child.id();
    // Reap in the short-lived launcher only if startup has already failed. The worker's
    // process group is independent; job state and /proc start time guard cancellation.
    if let Some(exit) = child.try_wait()? {
        bail!("worker failed to start: {exit}");
    }
    files::write_json(
        &engine.data.join("active-job.json"),
        &json!({"id":id,"pid":pid}),
    )?;
    Ok(json!({"id":id,"state":"running","pid":pid,"result":null,"error":null,"output":""}))
}
fn process_token(pid: u32) -> Option<String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)
        .map(str::to_owned)
}
pub fn status(engine: &Engine, id: &str) -> Result<Value> {
    let dir = task_dir(engine, id)?;
    let mut state = files::json_file(&dir.join("state.json"))?;
    if state["id"] != id {
        bail!("task not found");
    }
    if state["state"] == "running"
        && let (Some(pid), Some(token)) = (state["pid"].as_u64(), state["process_token"].as_str())
        && (cfg!(target_os = "linux") || cfg!(target_os = "android"))
        && process_token(pid as u32).as_deref() != Some(token)
    {
        state["state"] = json!("failed");
        state["error"] = json!("worker exited before completion");
        files::write_json(&dir.join("state.json"), &state)?;
    }
    if state["state"] == "running"
        && state["pid"].is_null()
        && state["created"].as_u64().is_some_and(|created| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|now| now.as_secs().saturating_sub(created) > 5)
                .unwrap_or(false)
        })
    {
        state["state"] = json!("failed");
        state["error"] = json!("worker did not start");
        files::write_json(&dir.join("state.json"), &state)?;
    }
    use std::io::{Read, Seek, SeekFrom};
    let mut output = Vec::new();
    if let Ok(mut file) = fs::File::open(dir.join("output.log")) {
        let length = file.metadata()?.len();
        file.seek(SeekFrom::Start(length.saturating_sub(65536)))?;
        file.take(65536).read_to_end(&mut output)?;
    }
    state["output"] = json!(String::from_utf8_lossy(&output));
    Ok(state)
}
pub fn cancel(engine: &Engine, id: &str) -> Result<Value> {
    let dir = task_dir(engine, id)?;
    let mut state = status(engine, id)?;
    if state["state"] == "running" {
        // The worker polls this file, terminates its child and records the terminal state.
        files::atomic(&dir.join("cancel"), b"")?;
        state["cancel_requested"] = json!(true);
    }
    Ok(state)
}
pub fn cancel_active(engine: &Engine) -> Result<()> {
    let active = files::json_file(&engine.data.join("active-job.json"))?;
    if let Some(id) = active["id"].as_str() {
        cancel(engine, id)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        while status(engine, id)?["state"] == "running" {
            if Instant::now() >= deadline {
                bail!("generator cancellation pending; stop gate remains active");
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(())
}
pub fn work(engine: &Engine, id: &str) -> Result<Value> {
    let dir = task_dir(engine, id)?;
    let input = files::json_file(&dir.join("request.json"))?;
    let mut state = json!({"id":id,"state":"running","pid":std::process::id(),"process_token":process_token(std::process::id()),"result":null,"error":null});
    files::write_json(&dir.join("state.json"), &state)?;
    let result = (|| -> Result<Value> {
        if crate::power::disabled(engine) || dir.join("cancel").exists() {
            bail!("cancelled");
        }
        for name in ["autopif4.sh", "migrate.sh"] {
            fs::copy(engine.module.join("scripts").join(name), dir.join(name))?;
        }
        let mut log_options = fs::OpenOptions::new();
        log_options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            log_options.mode(0o600);
        }
        let output = log_options.open(dir.join("output.log"))?;
        let executable = std::env::current_exe()?;
        let mut cmd = engine.shell();
        cmd.arg(dir.join("autopif4.sh"))
            .current_dir(&dir)
            .env("PIF_ENGINE_BIN", executable)
            .env("PIF_ENGINE_DATA", &engine.data)
            .stdout(Stdio::from(output.try_clone()?))
            .stderr(Stdio::from(output))
            .stdin(Stdio::null());
        if input["strong"].as_bool().unwrap_or(true) {
            cmd.arg("--strong");
        }
        if input["match_device"].as_bool().unwrap_or(false)
            && !engine.data.join("skipmatchdevice").exists()
        {
            cmd.arg("--match");
        }
        if crate::power::disabled(engine) || dir.join("cancel").exists() {
            bail!("cancelled");
        }
        let mut child = Process::spawn(&mut cmd)?;
        let seconds = input["timeout_seconds"].as_u64().unwrap_or(120);
        let deadline = Instant::now() + Duration::from_secs(seconds);
        loop {
            if crate::power::disabled(engine) {
                files::atomic(&dir.join("cancel"), b"")?;
            }
            if dir.join("cancel").exists() || Instant::now() >= deadline {
                if dir.join("cancel").exists() {
                    bail!("cancelled");
                }
                bail!("generation timed out after {seconds} seconds");
            }
            if let Some(exit) = child.try_wait()? {
                if !exit.success() {
                    bail!("generator exited with {exit}");
                }
                break;
            }
            if fs::metadata(dir.join("output.log"))
                .map(|m| m.len())
                .unwrap_or(0)
                > 1024 * 1024
            {
                bail!("generator output exceeded 1 MiB");
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        child.terminate();
        if fs::metadata(dir.join("output.log"))?.len() > 1024 * 1024 {
            bail!("generator output exceeded 1 MiB");
        }
        let content = files::read(&dir.join("custom.pif.prop"))?;
        if content.trim().is_empty() {
            bail!("generator produced no configuration");
        }
        let mut entries = config::parse(&content, "prop")?;
        // Canary retains upstream's explicitly selected compatibility heuristic.
        entries
            .entry("ro.board.first_api_level".into())
            .or_insert("32".into());
        config::migrate(&mut entries, true, false);
        let previous = config::current(engine)?;
        for (key, _) in config::ADVANCED {
            if let Some(value) = previous["entries"][*key].as_str() {
                entries.insert((*key).into(), value.into());
            }
        }
        if input["strong"].as_bool().unwrap_or(true) {
            entries.insert("spoofProvider".into(), "0".into());
        }
        let mut result = config::preview(
            engine,
            &json!({"content":config::serialize(&entries,"json")?,"format":"json"}),
        )?;
        result["warnings"]
            .as_array_mut()
            .unwrap()
            .push(json!({"code":"canary","fields":[]}));
        result["metadata"] = json!({"released":comment_value(&content,"Canary Released:"),"estimated_expiry":comment_value(&content,"Estimated Expiry:")});
        Ok(result)
    })();
    if crate::power::disabled(engine) || dir.join("cancel").exists() {
        state["state"] = json!("cancelled");
        state["error"] = json!("cancelled");
    } else {
        match result {
            Ok(value) => {
                state["state"] = json!("completed");
                state["result"] = value;
            }
            Err(error) => {
                state["state"] = json!(if dir.join("cancel").exists() {
                    "cancelled"
                } else {
                    "failed"
                });
                state["error"] = json!(format!("{error:#}"));
            }
        }
    }
    // Keep the terminal result and a bounded log; downloaded pages and build scratch
    // files are disposable once the candidate has been recorded.
    if let Ok(mut log) = fs::File::open(dir.join("output.log")) {
        use std::io::{Read, Seek, SeekFrom};
        let length = log.metadata()?.len();
        log.seek(SeekFrom::Start(length.saturating_sub(65536)))?;
        let mut tail = Vec::new();
        log.take(65536).read_to_end(&mut tail)?;
        files::atomic(&dir.join("output.log"), &tail)?;
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name();
        if !["state.json", "output.log", "request.json"]
            .iter()
            .any(|keep| name == *keep)
        {
            let _ = fs::remove_file(entry.path());
        }
    }
    files::write_json(&dir.join("state.json"), &state)?;
    Ok(state)
}
fn comment_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.trim()
            .strip_prefix('#')?
            .trim()
            .strip_prefix(key)
            .map(|s| s.trim().to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_scoped_and_unknown_tasks_are_errors() {
        let root = tempfile::tempdir().unwrap();
        let e = Engine {
            data: root.path().into(),
            module: root.path().into(),
            sysroot: root.path().into(),
        };
        assert!(task_dir(&e, "../../x").is_err());
        assert!(status(&e, "missing").is_err());
        let dir = task_dir(&e, "test").unwrap();
        files::write_json(
            &dir.join("state.json"),
            &json!({"id":"test","state":"running"}),
        )
        .unwrap();
        assert_eq!(cancel(&e, "test").unwrap()["cancel_requested"], true);
        assert!(dir.join("cancel").exists());
    }
}
