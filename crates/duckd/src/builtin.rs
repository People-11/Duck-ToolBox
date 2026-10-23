//! Commands that need the whole feature registry rather than one feature.

use duck_core::{
    Feature, FeatureInfo,
    command::{CommandOutput, CommandResult},
    envelope,
};
use serde::Serialize;

pub const MANIFEST: &str = "features";

#[derive(Debug, Serialize)]
struct Manifest {
    binary_version: &'static str,
    api: u32,
    features: Vec<FeatureInfo>,
}

pub fn subcommand() -> clap::Command {
    clap::Command::new(MANIFEST).about("List the features compiled into this binary")
}

pub fn manifest(features: &[&dyn Feature]) -> CommandResult {
    CommandOutput::new(
        MANIFEST,
        Manifest {
            binary_version: env!("CARGO_PKG_VERSION"),
            api: envelope::API_VERSION,
            features: features.iter().map(|feature| feature.info()).collect(),
        },
    )
}
