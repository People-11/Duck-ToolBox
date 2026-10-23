//! Keybox renewal: keeps the active keystore module's keybox valid with keyboxes issued by
//! this device's KeyMint TA, and reports the state in the module's `module.prop`.

use std::path::Path;

use anyhow::{Result, bail};
use clap::{Arg, ArgAction};
use duck_core::{
    Context, Feature, IntoCommandResult, command::CommandResult, fs::write_string_atomic,
    internal_error, unix_now,
};
use serde::{Deserialize, Serialize};

pub const COMMAND: &str = "renew";
/// Renew once the earliest certificate expires within this window.
const RENEW_BEFORE: u64 = 24 * 60 * 60;
/// At most one automatic issuance per hour, whatever the outcome.
const RETRY_AFTER: u64 = 60 * 60;
const STATE_FILE: &str = "renew.json";
const MAX_ERROR_CHARS: usize = 160;

#[derive(Debug, Default, Serialize, Deserialize)]
struct State {
    attempted_at: u64,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct RenewData {
    /// Earliest certificate expiry of the installed keybox, in Unix seconds.
    expires_at: Option<u64>,
    renewed: bool,
    error: Option<String>,
    description: String,
}

pub fn subcommand() -> clap::Command {
    clap::Command::new(COMMAND)
        .about(
            "Issue a keybox through the device RKP issuer when the installed one is missing or \
             expires within a day, then write the state to module.prop",
        )
        .arg(
            Arg::new("force")
                .long("force")
                .action(ArgAction::SetTrue)
                .help("Issue now, regardless of expiry and the hourly retry limit"),
        )
}

pub async fn run(features: &[&dyn Feature], ctx: &Context, force: bool) -> CommandResult {
    renew(features, ctx, force)
        .await
        .into_command(COMMAND, internal_error)
}

async fn renew(features: &[&dyn Feature], ctx: &Context, force: bool) -> Result<RenewData> {
    let now = unix_now();
    let state_path = ctx.paths.var_dir.join(STATE_FILE);
    let mut state: State = duck_core::fs::read_optional(&state_path)?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();

    // An error here means there is no keystore module to install into.
    let mut expiry = duck_tricky_store::keybox::installed_expiry(ctx);
    let has_backend = expiry.is_ok();
    let due =
        matches!(expiry, Ok(None)) || matches!(expiry, Ok(Some(at)) if now + RENEW_BEFORE >= at);
    let allowed = now >= state.attempted_at + RETRY_AFTER;
    let mut renewed = false;
    if has_backend && (force || (due && allowed)) {
        state = match issue(ctx).await {
            Ok(expires_at) => {
                renewed = true;
                expiry = Ok(expires_at);
                State {
                    attempted_at: now,
                    error: None,
                }
            }
            Err(error) => State {
                attempted_at: now,
                error: Some(format!(
                    "{}: {}",
                    duck_rkp_device::code_of(&error),
                    truncate(&format!("{error:#}"))
                )),
            },
        };
        ctx.paths.ensure_runtime_dirs()?;
        write_string_atomic(&state_path, &serde_json::to_string(&state)?)?;
    }

    let expires_at = expiry.ok().flatten();
    // Without a keystore module the Tricky Store status line explains what is missing.
    let keybox = has_backend.then(|| keybox_line(now, expires_at, &state));
    let description = keybox
        .into_iter()
        .chain(
            features
                .iter()
                .filter_map(|feature| feature.status_line(ctx)),
        )
        .collect::<Vec<_>>()
        .join(" | ");
    write_description(&ctx.paths.root, &description)?;

    if force && !renewed {
        bail!(
            "{}",
            state
                .error
                .as_deref()
                .unwrap_or("no keystore module to install the keybox into")
        );
    }
    Ok(RenewData {
        expires_at,
        renewed,
        error: state.error,
        description,
    })
}

async fn issue(ctx: &Context) -> Result<Option<u64>> {
    let xml = duck_rkp_device::issue_keybox(ctx).await?;
    duck_tricky_store::keybox::install(ctx, &xml, "rkp-device")?;
    duck_tricky_store::keybox::installed_expiry(ctx)
}

fn keybox_line(now: u64, expires_at: Option<u64>, state: &State) -> String {
    let subject = match expires_at {
        Some(at) if now + RENEW_BEFORE < at => {
            return format!(
                "✅ Keybox valid until {}, renews after {}",
                utc(at),
                utc(at - RENEW_BEFORE)
            );
        }
        Some(at) if now < at => format!("Keybox expires {}", utc(at)),
        Some(at) => format!("Keybox expired {}", utc(at)),
        None => "No usable keybox".to_owned(),
    };
    match &state.error {
        Some(error) => format!(
            "❌ {subject}; renewal failed ({error}), retry after {}",
            utc(state.attempted_at + RETRY_AFTER)
        ),
        None => format!("⏳ {subject}; renewal pending"),
    }
}

/// Replaces the `description=` line, writing only when it changed.
fn write_description(root: &Path, description: &str) -> Result<()> {
    let path = root.join("module.prop");
    let raw = std::fs::read_to_string(&path)?;
    let line = format!("description={}", description.replace(['\r', '\n'], " "));
    let mut lines: Vec<&str> = raw
        .lines()
        .filter(|line| !line.starts_with("description="))
        .collect();
    lines.push(&line);
    let updated = lines.join("\n") + "\n";
    if updated != raw {
        write_string_atomic(&path, &updated)?;
    }
    Ok(())
}

fn truncate(text: &str) -> String {
    match text.char_indices().nth(MAX_ERROR_CHARS) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

fn utc(seconds: u64) -> String {
    match time::OffsetDateTime::from_unix_timestamp(seconds as i64) {
        Ok(at) => format!(
            "{}-{:02}-{:02} {:02}:{:02} UTC",
            at.year(),
            u8::from(at.month()),
            at.day(),
            at.hour(),
            at.minute()
        ),
        Err(_) => seconds.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{RENEW_BEFORE, State, keybox_line, utc};

    const NOW: u64 = 1_792_731_163; // 2026-10-23 04:52:43 UTC

    #[test]
    fn describes_each_renewal_state() {
        let ok = State::default();
        let failed = State {
            attempted_at: NOW,
            error: Some("ta_rejected: status -18".into()),
        };
        let later = NOW + 7 * 24 * 60 * 60;

        assert_eq!(utc(NOW), "2026-10-23 04:52 UTC");
        assert_eq!(
            keybox_line(NOW, Some(later), &ok),
            format!(
                "✅ Keybox valid until {}, renews after {}",
                utc(later),
                utc(later - RENEW_BEFORE)
            )
        );
        assert_eq!(
            keybox_line(NOW, Some(NOW + 60), &ok),
            format!("⏳ Keybox expires {}; renewal pending", utc(NOW + 60))
        );
        assert_eq!(
            keybox_line(NOW, None, &failed),
            "❌ No usable keybox; renewal failed (ta_rejected: status -18), retry after 2026-10-23 05:52 UTC"
        );
    }
}
