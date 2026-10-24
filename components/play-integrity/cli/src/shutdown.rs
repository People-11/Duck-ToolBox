// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: fail-closed shutdown independent of configuration validation.
use crate::{Engine, denylist, files, power, selinux, stages, tasks};
use anyhow::Result;
use serde_json::{Value, json};
fn issue(issues: &mut Vec<Value>, step: &str, result: Result<()>) {
    if let Err(error) = result {
        issues.push(json!({"step":step,"message":format!("{error:#}")}));
    }
}
pub fn status(engine: &Engine) -> Result<Value> {
    let saved = files::json_file(&engine.data.join("shutdown.json")).unwrap_or_else(|_| json!({}));
    Ok(
        json!({"enabled":!power::disabled(engine),"disabled_by_marker":power::gated(engine),"stop_revision":files::hash(files::read(&engine.data.join("disabled"))?.as_bytes()),"cleanup_pending":saved["cleanup_pending"].as_bool().unwrap_or(false),"issues":saved["issues"].as_array().cloned().unwrap_or_default()}),
    )
}
pub fn stop(engine: &Engine) -> Result<Value> {
    // Publish the stop gate before reading any mutable or potentially corrupt state.
    files::private_dir(&engine.data)?;
    let token = format!(
        "{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        std::process::id()
    );
    files::atomic(&engine.data.join("disabled"), token.as_bytes())?;
    files::write_json(
        &engine.data.join("shutdown.json"),
        &json!({"cleanup_pending":true,"issues":[{"step":"cleanup","message":"shutdown cleanup is pending"}]}),
    )?;
    let mut issues = Vec::new();
    issue(&mut issues, "generation", tasks::cancel_active(engine));
    match files::Lock::acquire(&engine.data) {
        Ok(_lock) => {
            if let Ok(mut opts) = files::json_file(&engine.data.join("options.json"))
                && opts.is_object()
            {
                opts["enabled"] = json!(false);
                issue(
                    &mut issues,
                    "options",
                    files::write_json(&engine.data.join("options.json"), &opts),
                );
            }
            issue(&mut issues, "denylist", denylist::restore(engine));
            issue(&mut issues, "selinux", selinux::restore(engine));
            if engine.module.join("scripts/stage.sh").exists() {
                issue(
                    &mut issues,
                    "compatibility",
                    stages::run(engine, "cleanup", "full").map(|_| ()),
                );
            }
        }
        Err(error) => issue(&mut issues, "lifecycle", Err(error)),
    }
    let result = json!({"enabled":false,"disabled_by_marker":true,"stop_revision":files::hash(token.as_bytes()),"cleanup_pending":!issues.is_empty(),"issues":issues,"reboot_required":true,"process_restart_required":true});
    files::write_json(&engine.data.join("shutdown.json"), &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_does_not_require_valid_configuration_or_an_available_write_lock() {
        let root = tempfile::tempdir().unwrap();
        let e = Engine {
            data: root.path().join("data"),
            module: root.path().join("module"),
            sysroot: root.path().join("sys"),
        };
        files::atomic(&e.data.join("options.json"), b"{broken").unwrap();
        files::atomic(&e.data.join("custom.pif.json"), b"{also broken").unwrap();
        let lock = files::Lock::acquire(&e.data).unwrap();
        let result = stop(&e).unwrap();
        assert_eq!(result["enabled"], false);
        assert_eq!(result["cleanup_pending"], true);
        assert!(power::disabled(&e));
        assert_eq!(
            files::read(&e.data.join("custom.pif.json")).unwrap(),
            "{also broken"
        );
        drop(lock);
        assert_eq!(stop(&e).unwrap()["cleanup_pending"], false);
        assert_eq!(
            files::read(&e.data.join("options.json")).unwrap(),
            "{broken"
        );
    }
    #[test]
    fn stale_enable_cannot_override_a_later_stop() {
        let root = tempfile::tempdir().unwrap();
        let e = Engine {
            data: root.path().join("data"),
            module: root.path().join("module"),
            sysroot: root.path().join("sys"),
        };
        let first = stop(&e).unwrap();
        let second = stop(&e).unwrap();
        assert!(
            crate::operations::run(
                &e,
                "options",
                json!({"enabled":true,"expected_stop_revision":first["stop_revision"]})
            )
            .is_err()
        );
        assert_eq!(
            crate::operations::run(
                &e,
                "options",
                json!({"enabled":true,"expected_stop_revision":second["stop_revision"]})
            )
            .unwrap()["options"]["enabled"],
            true
        );
        assert!(!power::disabled(&e));
        // A stop between an enable's option write and acknowledgement still wins.
        files::atomic(&e.data.join("disabled"), b"newer-stop").unwrap();
        assert!(power::disabled(&e));
    }
}
