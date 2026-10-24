use std::io::{self, Read};

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use duck_core::{
    BoxFuture, ClapFeature, CommandResult, Context as RunContext, FeatureInfo, IntoCommandResult,
    feature::ready,
};

use crate::{DeviceIdsProfile, defaults, error::code_of, provision};

pub static FEATURE: ClapFeature<Command> = ClapFeature::new(
    FeatureInfo {
        id: "device-ids",
        summary: "Attestation device ID provisioning (Qualcomm / MediaTek TrustKernel)",
        contract: 2,
    },
    dispatch,
);

fn dispatch<'a>(command: Command, ctx: &'a RunContext) -> BoxFuture<'a, CommandResult> {
    ready(run(command, ctx))
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Print the identifiers detected from system properties.
    Defaults,
    /// Provision device IDs into the Keymaster trusted app (profile JSON on stdin).
    Provision(ProvisionArgs),
}

#[derive(Debug, Args, Clone)]
pub struct ProvisionArgs {
    #[arg(long = "stdin-json")]
    pub stdin_json: bool,
}

pub fn run(command: Command, ctx: &RunContext) -> CommandResult {
    match command {
        Command::Defaults => {
            anyhow::Ok(defaults::detect(&duck_platform::props::all(), &ctx.sysroot))
                .into_command("device-ids.defaults", code_of)
        }
        Command::Provision(args) => {
            provision_from_stdin(ctx, &args).into_command("device-ids.provision", code_of)
        }
    }
}

fn provision_from_stdin(
    ctx: &RunContext,
    args: &ProvisionArgs,
) -> Result<crate::DeviceIdsProvisionResult, duck_core::Failure> {
    if !args.stdin_json {
        return Err(anyhow::anyhow!("device-ids provision requires `--stdin-json`").into());
    }

    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .context("read device ID profile JSON from stdin")?;
    let profile =
        serde_json::from_str::<DeviceIdsProfile>(&input).context("parse device ID profile JSON")?;

    provision(ctx, profile)
}
