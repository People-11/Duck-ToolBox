//! MIT management adapter. The separately built GPL engine owns its implementation and state.
//! This crate communicates only over the engine's documented command-line interface.
mod process;
pub use process::request;

use anyhow::{Result, bail};
use clap::{Args, Subcommand};
use duck_core::{
    BoxFuture, ClapFeature, CommandFailure, CommandOutput, CommandResult, Context, FeatureInfo,
};
use serde_json::{Value, json};
use std::io::{self, Read};

pub static FEATURE: ClapFeature<Command> = ClapFeature::new(
    FeatureInfo {
        id: "play-integrity",
        summary: "Play Integrity engine status, fingerprint generation and configuration",
        contract: 1,
    },
    dispatch,
);

fn dispatch<'a>(command: Command, ctx: &'a Context) -> BoxFuture<'a, CommandResult> {
    Box::pin(async move { run(command, ctx) })
}

#[derive(Debug, Args)]
pub struct Input {
    #[arg(long = "stdin-json")]
    stdin_json: bool,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    Status,
    Config,
    Preview(Input),
    Apply(Input),
    Restore(Input),
    TaskStart,
    TaskStatus(Input),
    Restart,
}

fn run(command: Command, ctx: &Context) -> CommandResult {
    let (operation, label, input) = match command {
        Command::Status => ("status", "play-integrity.status", None),
        Command::Config => ("config", "play-integrity.config", None),
        Command::Preview(v) => ("preview", "play-integrity.preview", Some(v)),
        Command::Apply(v) => ("apply", "play-integrity.apply", Some(v)),
        Command::Restore(v) => ("restore", "play-integrity.restore", Some(v)),
        Command::TaskStart => ("task-start", "play-integrity.task-start", None),
        Command::TaskStatus(v) => ("task-status", "play-integrity.task-status", Some(v)),
        Command::Restart => ("restart", "play-integrity.restart", None),
    };
    let result = (|| -> Result<Value> {
        let input = if let Some(input) = input {
            read_input(&input)?
        } else {
            json!({})
        };
        process::request(ctx, operation, &input)
    })();
    match result {
        Ok(data) => CommandOutput::new(label, data),
        Err(error) => {
            let code = if ctx.paths.root.join("play-integrity/pif-engine").is_file() {
                "engine_error"
            } else {
                "engine_missing"
            };
            let details = error
                .downcast_ref::<process::EngineError>()
                .map(|e| e.0.clone());
            Err(CommandFailure::new(label, code, &error, details))
        }
    }
}
fn read_input(input: &Input) -> Result<Value> {
    if !input.stdin_json {
        bail!("JSON input requires --stdin-json");
    }
    let mut bytes = Vec::new();
    io::stdin().take(128 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        bail!("input exceeds 128 KiB");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
