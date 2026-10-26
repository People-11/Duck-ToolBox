//! One Play Integrity fingerprint refresh: generate a Pixel Canary candidate, apply it
//! together with the matching keystore security patch, then restart the PI processes.

use std::time::{Duration, Instant};

use anyhow::{Result, bail};
use duck_core::Context;
use duck_play_integrity::request;
use serde_json::{Value, json};

/// The engine stops its generator after 120 seconds; allow for start-up and clean-up.
const GENERATION_LIMIT: Duration = Duration::from_secs(150);

/// Whether the engine has no fingerprint at all, so it injects nothing yet.
pub fn missing(ctx: &Context) -> Result<bool> {
    let config = request(ctx, "config", &json!({}))?;
    Ok(config["entries"].as_object().is_none_or(|e| e.is_empty()))
}

/// The applied fingerprint's model and security patch, for the module description.
pub fn summary(ctx: &Context) -> Option<String> {
    let config = request(ctx, "config", &json!({})).ok()?;
    let entries = &config["entries"];
    let model = entries["MODEL"].as_str()?;
    Some(match entries["SECURITY_PATCH"].as_str() {
        Some(patch) => format!("{model} ({patch})"),
        None => model.to_owned(),
    })
}

/// `sync_patch` aligns the active keystore module's security patch policy with the fingerprint.
pub fn refresh(ctx: &Context, sync_patch: bool) -> Result<()> {
    let candidate = generate(ctx)?;
    let patch = match candidate["suggested_patch"].as_str().filter(|_| sync_patch) {
        Some(patch) => {
            let preview = duck_tricky_store::patch::preview(
                ctx,
                &duck_tricky_store::patch::Request {
                    patch: patch.into(),
                    ..Default::default()
                },
            )?;
            json!({"target":"tricky-store","patch":preview["patch"],"revision":preview["revision"]})
        }
        None => Value::Null,
    };
    let engine =
        json!({"content":candidate["json"],"format":"json","revision":candidate["revision"]});
    crate::integrity_transaction::apply_unattended(ctx, &json!({"engine":engine,"patch":patch}))?;
    // The configuration is saved either way; a process that survives picks it up later.
    let _ = request(ctx, "restart", &json!({}));
    Ok(())
}

fn generate(ctx: &Context) -> Result<Value> {
    let started = request(ctx, "task-start", &json!({}))?;
    let id = started["id"].clone();
    let deadline = Instant::now() + GENERATION_LIMIT;
    loop {
        let task = request(ctx, "task-status", &json!({"id":id}))?;
        match task["state"].as_str() {
            Some("completed") if task["result"].is_object() => return Ok(task["result"].clone()),
            Some("running") if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_secs(1))
            }
            Some("running") => bail!("fingerprint generation did not finish"),
            _ => bail!(
                "fingerprint generation failed: {}",
                task["error"].as_str().unwrap_or("unknown error")
            ),
        }
    }
}
