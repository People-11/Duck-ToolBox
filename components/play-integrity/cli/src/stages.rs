// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: root-manager shell adapter shared by boot and explicit shutdown.
use crate::{Engine, command};
use anyhow::{Result, bail};
use std::time::Duration;
pub fn run(engine: &Engine, phase: &str, mode: &str) -> Result<String> {
    let ksu = engine.device("/data/adb/ksud").exists() || engine.device("/data/adb/ksu").exists();
    let apatch =
        !ksu && (engine.device("/data/adb/apd").exists() || engine.device("/data/adb/ap").exists());
    let output = command::run(
        engine
            .shell()
            .env("KSU", if ksu { "true" } else { "false" })
            .env("APATCH", if apatch { "true" } else { "false" })
            .arg(engine.module.join("scripts/stage.sh"))
            .arg(phase)
            .env("PIF_ENGINE_DATA", &engine.data)
            .env("PIF_ENGINE_SCRIPTS", engine.module.join("scripts"))
            .env(
                "PIF_ENGINE_ROOT",
                engine.module.parent().unwrap_or(&engine.module),
            )
            .env("PIF_ENGINE_MODE", mode),
        Duration::from_secs(if ["cleanup", "uninstall"].contains(&phase) {
            5
        } else {
            30
        }),
    )?;
    if !output.status.success() {
        bail!(
            "stage {phase} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
