// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: standalone engine operations; no toolbox runtime dependency.
// Modified 2026-10-26: fixed default options (full mode, no skips, bundled replacement list);
// no stop control, configuration library or WebUI-only operations, no reboot-evidence files,
// and only the newest configuration backup is kept.
use crate::{Engine, config, files, tasks, text};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::process::Command;

pub const UPSTREAM: &str = "23ec56c2440c4fd1a5e893a23ef7861d6366a828";
pub fn conflicts(engine: &Engine) -> Vec<String> {
    ["playintegrityfix", "safetynet-fix"]
        .into_iter()
        .filter(|id| {
            let path = engine.device(&format!("/data/adb/modules/{id}"));
            path.join("module.prop").exists()
                && !path.join("disable").exists()
                && !path.join("remove").exists()
        })
        .map(str::to_owned)
        .collect()
}
pub fn run(engine: &Engine, operation: &str, input: Value) -> Result<Value> {
    match operation {
        "status" => status(engine),
        "config" => config::current(engine),
        "preview" => config::preview(engine, &input),
        "prepare" => prepare(engine, &input),
        "apply" => apply(engine, &input),
        "restore" => restore(engine, &input),
        "task-start" => tasks::start(engine),
        "task-status" => tasks::status(engine, text(&input, "id")?),
        "restart" => restart(engine),
        _ => bail!("unknown operation: {operation}"),
    }
}
fn replacement_list(engine: &Engine) -> Result<String> {
    files::read(&engine.module.join("scripts/app_replace_list.txt"))
}
fn module_enabled(engine: &Engine) -> bool {
    let root = engine.module.parent().unwrap_or(&engine.module);
    !root.join("disable").exists() && !root.join("remove").exists()
}
pub fn active(engine: &Engine) -> Result<bool> {
    Ok(module_enabled(engine) && conflicts(engine).is_empty() && configured(engine)?)
}
fn configured(engine: &Engine) -> Result<bool> {
    let current = config::current(engine)?;
    Ok(current["entries"]
        .as_object()
        .is_some_and(|m| !m.is_empty()))
}
pub fn status(engine: &Engine) -> Result<Value> {
    let conflict = conflicts(engine);
    let has_config = configured(engine)?;
    // Magisk's installation directory proves neither the setting nor a loaded
    // Zygisk runtime. Its boot-script environment is supplied by the daemon.
    let magisk_runtime = std::env::var("ZYGISK_ENABLED").as_deref() == Ok("1");
    let zygisk = ["zygisksu", "rezygisk", "neozygisk"].iter().any(|id| {
        let path = engine.device(&format!("/data/adb/modules/{id}"));
        path.join("module.prop").exists()
            && !path.join("disable").exists()
            && !path.join("remove").exists()
    }) || magisk_runtime;
    let runtime_active = active(engine)?;
    let state = if !module_enabled(engine) {
        "disabled"
    } else if !conflict.is_empty() {
        "conflict"
    } else if !has_config {
        "configuration-required"
    } else if !zygisk {
        "zygisk-unconfirmed"
    } else {
        "configured"
    };
    Ok(
        json!({"api":1,"upstream_revision":UPSTREAM,"has_config":has_config,"lifecycle_eligible":runtime_active,"zygisk_detected":zygisk,
        "conflicts":conflict,"state":state,
        "data_dir":engine.data,"source":"licenses/play-integrity/SOURCE.txt","license":"licenses/play-integrity/GPL-3.0.txt","verdict_verified":false}),
    )
}
fn prepare(engine: &Engine, input: &Value) -> Result<Value> {
    let _lock = files::Lock::acquire(&engine.data)?;
    let preview = config::preview(engine, input)?;
    if input["revision"] != preview["revision"] {
        bail!("configuration changed; preview again");
    }
    let old = files::read(&engine.data.join("custom.pif.json"))?;
    let new = text(&preview, "json")?;
    let token = files::hash(format!("{old}\0{new}").as_bytes());
    // A new application supersedes every earlier backup.
    let _ = std::fs::remove_dir_all(engine.data.join("backups"));
    files::write_json(
        &engine.data.join("backups").join(format!("{token}.json")),
        &json!({"content":old,"applied_hash":files::hash(new.as_bytes())}),
    )?;
    Ok(json!({"backup":token}))
}
fn apply(engine: &Engine, input: &Value) -> Result<Value> {
    let _lock = files::Lock::acquire(&engine.data)?;
    let preview = config::preview(engine, input)?;
    if input["revision"] != preview["revision"] {
        bail!("configuration changed; preview again");
    }
    let path = engine.data.join("custom.pif.json");
    let old = files::read(&path)?;
    let new = text(&preview, "json")?;
    let token = files::hash(format!("{old}\0{new}").as_bytes());
    match input.get("backup") {
        Some(backup) if backup != &token => bail!("prepared configuration changed; preview again"),
        Some(_) => {}
        // Not prepared: this application supersedes every earlier backup.
        None => {
            let _ = std::fs::remove_dir_all(engine.data.join("backups"));
        }
    }
    let backup = engine.data.join("backups").join(format!("{token}.json"));
    files::write_json(
        &backup,
        &json!({"content":old,"applied_hash":files::hash(new.as_bytes())}),
    )?;
    let publication = (|| -> Result<Value> {
        files::atomic(&path, new.as_bytes())?;
        let saved = config::current(engine)?;
        if saved["revision"] != files::hash(new.as_bytes()) {
            bail!("configuration readback failed");
        }
        Ok(saved)
    })();
    match publication {
        Ok(saved) => Ok(json!({"config":saved,"backup":token,"restart_required":true})),
        Err(error) => {
            if let Err(recovery) = files::atomic(&path, old.as_bytes()) {
                return Err(files::RecoveryError(json!({"path":path,"backup_path":backup,"apply_error":format!("{error:#}"),"restore_error":format!("{recovery:#}")})).into());
            }
            Err(error.context("publication failed; original configuration restored"))
        }
    }
}
fn restore(engine: &Engine, input: &Value) -> Result<Value> {
    let _lock = files::Lock::acquire(&engine.data)?;
    let token = text(input, "backup")?;
    if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid backup token");
    }
    let backup = engine.data.join("backups").join(format!("{token}.json"));
    let saved = files::json_file(&backup)?;
    let path = engine.data.join("custom.pif.json");
    let current = files::read(&path)?;
    if current != text(&saved, "content")?
        && saved["applied_hash"] != files::hash(current.as_bytes())
    {
        bail!("configuration changed after apply; restore refused");
    }
    files::atomic(&path, text(&saved, "content")?.as_bytes()).map_err(|e| {
        files::RecoveryError(json!({"path":path,"backup_path":backup,"reason":e.to_string()}))
    })?;
    Ok(json!({"config":config::current(engine)?,"restart_required":true}))
}
fn restart(engine: &Engine) -> Result<Value> {
    if engine.sysroot != std::path::Path::new("/") {
        bail!("process restart requires a live device");
    }
    let mut results = Vec::new();
    for process in ["com.google.android.gms.unstable", "com.android.vending"] {
        let output = Command::new("killall").arg(process).output()?;
        results.push(json!({"process":process,"exit_code":output.status.code(),"output":String::from_utf8_lossy(&output.stderr)}));
    }
    Ok(json!({"processes":results,"verdict_verified":false}))
}
pub fn stage(engine: &Engine, phase: &str) -> Result<Value> {
    if !["early", "late", "setup", "uninstall"].contains(&phase) {
        bail!("invalid lifecycle phase");
    }
    let _lock = files::Lock::acquire(&engine.data)?;
    if phase == "uninstall" {
        crate::denylist::restore(engine)?;
        crate::selinux::restore(engine)?;
        return Ok(json!({"stage":phase,"output":crate::stages::run(engine,"uninstall")?}));
    }
    let active = active(engine)?;
    let script_phase = if phase == "early" && !active {
        "cleanup"
    } else {
        phase
    };
    if phase != "early" && !active {
        return Ok(json!({"skipped":true}));
    }
    if active && ["setup", "early"].contains(&phase) {
        crate::paths::validate_list(&replacement_list(engine)?)?;
    }
    files::private_dir(&engine.data)?;
    if phase == "early" && active {
        crate::denylist::manage(engine)?;
    }
    if script_phase == "cleanup" {
        crate::denylist::restore(engine)?;
        crate::selinux::restore(engine)?;
    }
    let output = crate::stages::run(engine, script_phase)?;
    if phase == "late" && active && !engine.device("/data/adb/disable_prop_handler").exists() {
        crate::selinux::apply(engine)?;
    }
    Ok(json!({"stage":phase,"output":output}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    fn engine(root: &std::path::Path) -> Engine {
        Engine {
            data: root.join("data"),
            module: root.join("module/play-integrity"),
            sysroot: root.join("sys"),
        }
    }
    #[test]
    fn root_manager_flags_and_missing_config_release_scripts() {
        let temporary = tempfile::tempdir().unwrap();
        let engine = engine(temporary.path());
        files::atomic(
            &engine.module.join("scripts/stage.sh"),
            b"printf '%s' \"$1\"\n",
        )
        .unwrap();
        assert!(!active(&engine).unwrap());
        assert_eq!(stage(&engine, "early").unwrap()["output"], "cleanup");
        assert_eq!(stage(&engine, "setup").unwrap()["skipped"], true);
        files::atomic(
            &engine.data.join("custom.pif.json"),
            br#"{"MODEL":"Fixture"}"#,
        )
        .unwrap();
        assert!(active(&engine).unwrap());
        for flag in ["disable", "remove"] {
            let path = engine.module.parent().unwrap().join(flag);
            files::atomic(&path, b"").unwrap();
            assert!(!active(&engine).unwrap());
            assert_eq!(stage(&engine, "early").unwrap()["output"], "cleanup");
            assert_eq!(stage(&engine, "late").unwrap()["skipped"], true);
            fs::remove_file(path).unwrap();
            assert!(active(&engine).unwrap());
        }
    }
    #[test]
    fn prepared_backup_is_guarded_and_recovery_is_repeatable() {
        let root = tempfile::tempdir().unwrap();
        let e = engine(root.path());
        let mut input = json!({"content":"MODEL=A","format":"prop"});
        input["revision"] = config::preview(&e, &input).unwrap()["revision"].clone();
        let saved = prepare(&e, &input).unwrap();
        input["backup"] = json!("wrong");
        assert!(apply(&e, &input).is_err());
        assert_eq!(config::current(&e).unwrap()["entries"], json!({}));
        input["backup"] = saved["backup"].clone();
        apply(&e, &input).unwrap();
        restore(&e, &saved).unwrap();
        restore(&e, &saved).unwrap();
        let mut next = json!({"content":"MODEL=B","format":"prop"});
        next["revision"] = config::preview(&e, &next).unwrap()["revision"].clone();
        apply(&e, &next).unwrap();
        let backups = fs::read_dir(e.data.join("backups")).unwrap().count();
        assert_eq!(backups, 1, "only the newest backup is kept");
    }
    #[test]
    fn preview_apply_stale_revision_and_restore() {
        let root = tempfile::tempdir().unwrap();
        let e = engine(root.path());
        let mut input = json!({"content":"MODEL=A\nCUSTOM=x","format":"prop"});
        let p = config::preview(&e, &input).unwrap();
        input["revision"] = p["revision"].clone();
        let applied = apply(&e, &input).unwrap();
        assert_eq!(config::current(&e).unwrap()["entries"]["MODEL"], "A");
        assert!(apply(&e, &input).is_err());
        restore(&e, &json!({"backup":applied["backup"]})).unwrap();
        assert_eq!(config::current(&e).unwrap()["entries"], json!({}));
        assert!(restore(&e, &json!({"backup":"../escape"})).is_err());
    }
}
