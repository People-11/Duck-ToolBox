// SPDX-License-Identifier: GPL-3.0-only
// Play Integrity Fork derivative; see the engine LICENSE and UPSTREAM.json.
// Modified 2026-10-24: independent CLI and versioned process interface.
// Modified 2026-10-26: no standalone migration or activity probe commands.
mod command;
mod config;
mod denylist;
mod files;
mod network;
mod operations;
mod paths;
mod rules;
mod selinux;
mod stages;
mod tasks;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[derive(Parser)]
#[command(
    version,
    about = "Play Integrity engine: standalone configuration and runtime tools (GPLv3)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Operations: status, config, preview, prepare, apply, restore, task-start,
    /// task-status, restart. Optional JSON input on stdin.
    Request { operation: String },
    /// Run an installation, early-boot, late-boot or uninstall lifecycle stage.
    Stage { phase: String },
    /// HTTPS fetch adapter for the bundled fingerprint generator.
    Fetch {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    #[command(hide = true)]
    Worker { id: String },
}

#[derive(Clone)]
pub struct Engine {
    pub data: PathBuf,
    pub module: PathBuf,
    pub sysroot: PathBuf,
}

impl Engine {
    fn discover() -> Result<Self> {
        let sysroot = std::env::var_os("PIF_ENGINE_SYSROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|| "/".into());
        let module = std::env::var_os("PIF_ENGINE_MODULE")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data/adb/modules/duck-toolbox/play-integrity"));
        let data = std::env::var_os("PIF_ENGINE_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| sysroot.join("data/adb/play-integrity"));
        if !data.is_absolute() || !module.is_absolute() || !sysroot.is_absolute() {
            bail!("engine paths must be absolute");
        }
        Ok(Self {
            data,
            module,
            sysroot,
        })
    }
    pub fn shell(&self) -> std::process::Command {
        for path in [
            "/data/adb/ksu/bin/busybox",
            "/data/adb/ap/bin/busybox",
            "/data/adb/magisk/busybox",
        ] {
            let path = self.device(path);
            if path.is_file() {
                let mut shell = std::process::Command::new(path);
                shell.arg("sh").env("ASH_STANDALONE", "1");
                return shell;
            }
        }
        std::process::Command::new("sh")
    }
    pub fn device(&self, path: &str) -> PathBuf {
        self.sysroot.join(path.trim_start_matches('/'))
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Command::Fetch { args } = cli.command {
        if let Err(error) = network::fetch(&args).await {
            eprintln!("{error:#}");
            std::process::exit(1);
        }
        return;
    }
    let result = async {
        let engine = Engine::discover()?;
        match cli.command {
            Command::Worker { id } => tasks::work(&engine, &id),
            Command::Stage { phase } => operations::stage(&engine, &phase),
            Command::Request { operation } => {
                let mut input = String::new();
                io::stdin()
                    .take(128 * 1024 + 1)
                    .read_to_string(&mut input)?;
                if input.len() > 128 * 1024 {
                    bail!("input exceeds 128 KiB");
                }
                let request = if input.trim().is_empty() {
                    json!({})
                } else {
                    serde_json::from_str(&input)?
                };
                operations::run(&engine, &operation, request)
            }
            Command::Fetch { .. } => unreachable!(),
        }
    }
    .await;
    match result {
        Ok(data) => println!("{}", json!({"ok":true,"api":1,"data":data})),
        Err(error) => {
            let mut failure = json!({"code":"engine_error","message":format!("{error:#}")});
            if let Some(details) = error.downcast_ref::<files::RecoveryError>() {
                failure["details"] = details.0.clone();
            }
            println!("{}", json!({"ok":false,"api":1,"error":failure}));
            std::process::exit(1);
        }
    }
}

pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing string: {key}"))
}
