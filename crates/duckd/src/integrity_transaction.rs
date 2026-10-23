//! Application composition: durable recovery across independently owned configurations.
use anyhow::{Context as _, Result, bail};
use clap::{Args, Subcommand};
use duck_core::{BoxFuture, ClapFeature, CommandResult, Context, FeatureInfo, IntoCommandResult};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{fs, io::Read, path::PathBuf};

pub static FEATURE: ClapFeature<Command> = ClapFeature::new(
    FeatureInfo {
        id: "integrity-transaction",
        summary: "Durable configuration application and confirmed recovery",
        contract: 1,
    },
    dispatch,
);
#[derive(Debug, Subcommand)]
pub enum Command {
    Status,
    Apply(Input),
    Recover(Input),
}
#[derive(Debug, Args)]
pub struct Input {
    #[arg(long = "stdin-json")]
    stdin_json: bool,
}
#[derive(Serialize, Deserialize)]
struct Journal {
    api: u32,
    phase: String,
    engine_backup: String,
    patch_backup: Option<String>,
    engine_started: bool,
    patch_started: bool,
    #[serde(default)]
    restart_required: bool,
}
impl Journal {
    fn pending(&self) -> bool {
        self.phase == "applying" || self.phase == "recovering"
    }
}
fn dispatch<'a>(command: Command, ctx: &'a Context) -> BoxFuture<'a, CommandResult> {
    Box::pin(async move {
        let label = match &command {
            Command::Status => "integrity-transaction.status",
            Command::Apply(_) => "integrity-transaction.apply",
            Command::Recover(_) => "integrity-transaction.recover",
        };
        run(ctx, command).into_command(label, duck_core::internal_error)
    })
}
fn input(args: Input) -> Result<Value> {
    if !args.stdin_json {
        bail!("JSON input requires --stdin-json");
    }
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 128 * 1024 {
        bail!("input exceeds 128 KiB");
    }
    Ok(serde_json::from_slice(&bytes)?)
}
struct Store {
    path: PathBuf,
    _lock: fs::File,
}
impl Store {
    fn open(ctx: &Context) -> Result<Self> {
        let dir = ctx.paths.var_dir.join("integrity-transaction");
        fs::create_dir_all(&dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        }
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("lock"))?;
        lock.try_lock()
            .context("configuration transaction is busy")?;
        Ok(Self {
            path: dir.join("journal.json"),
            _lock: lock,
        })
    }
    fn load(&self) -> Result<Option<Journal>> {
        let Some(raw) = duck_core::fs::read_optional(&self.path)? else {
            return Ok(None);
        };
        let journal: Journal = serde_json::from_str(&raw)?;
        if journal.api != 1
            || !["applying", "recovering", "completed", "restored"]
                .contains(&journal.phase.as_str())
        {
            bail!("unsupported transaction journal: {}", self.path.display());
        }
        Ok(Some(journal))
    }
    fn save(&self, journal: &Journal) -> Result<()> {
        duck_core::fs::write_string_atomic(&self.path, &serde_json::to_string(journal)?)?;
        fs::File::open(self.path.parent().context("no journal directory")?)?.sync_all()?;
        Ok(())
    }
}
fn token(value: &Value) -> Result<String> {
    Ok(value["backup"]
        .as_str()
        .context("participant omitted backup token")?
        .to_owned())
}
fn recover(ctx: &Context, store: &Store, journal: &mut Journal) -> Result<Value> {
    journal.phase = "recovering".into();
    store.save(journal)?;
    let mut failures = Vec::new();
    let mut restart = journal.restart_required;
    if journal.patch_started {
        let result = patch_restore(
            ctx,
            journal
                .patch_backup
                .as_deref()
                .context("missing patch backup")?,
        );
        match result {
            Ok(value) => {
                restart |= value["restart_required"] == true;
                journal.restart_required = restart;
                journal.patch_started = false;
                store.save(journal)?;
            }
            Err(error) => failures.push(format!(
                "keystore: {error:#}; backup={:?}",
                journal.patch_backup
            )),
        }
    }
    if journal.engine_started {
        match duck_play_integrity::request(ctx, "restore", &json!({"backup":journal.engine_backup}))
        {
            Ok(value) => {
                restart |= value["restart_required"] == true;
                journal.restart_required = restart;
                journal.engine_started = false;
                store.save(journal)?;
            }
            Err(error) => failures.push(format!(
                "Play Integrity: {error:#}; backup={}",
                journal.engine_backup
            )),
        }
    }
    if !failures.is_empty() {
        bail!("{}; journal: {}", failures.join("; "), store.path.display());
    }
    journal.phase = "restored".into();
    store.save(journal)?;
    Ok(json!({"restored":true,"restart_required":restart}))
}
fn run(ctx: &Context, command: Command) -> Result<Value> {
    let store = Store::open(ctx)?;
    let saved = store.load()?;
    match command {
        Command::Status => Ok(
            json!({"pending":saved.as_ref().is_some_and(Journal::pending),"journal_path":store.path,"journal":saved}),
        ),
        Command::Recover(args) => {
            if input(args)?["confirmed"] != true {
                bail!("recovery requires explicit confirmation");
            }
            let mut journal = saved.context("no configuration transaction to recover")?;
            if !journal.pending() {
                bail!("no interrupted transaction to recover");
            }
            recover(ctx, &store, &mut journal)
        }
        Command::Apply(args) => {
            if saved.as_ref().is_some_and(Journal::pending) {
                bail!(
                    "interrupted transaction requires confirmed recovery; journal: {}",
                    store.path.display()
                );
            }
            let request = input(args)?;
            let candidate = request.get("engine").context("missing engine preview")?;
            let stop_revision =
                duck_play_integrity::request(ctx, "power", &json!({}))?["stop_revision"].clone();
            let engine_backup = token(&duck_play_integrity::request(ctx, "prepare", candidate)?)?;
            let patch = request.get("patch").filter(|v| !v.is_null());
            let patch_backup = patch
                .map(|v| patch_prepare(ctx, v).and_then(|v| token(&v)))
                .transpose()?;
            let mut journal = Journal {
                api: 1,
                phase: "applying".into(),
                engine_backup,
                patch_backup,
                engine_started: false,
                patch_started: false,
                restart_required: false,
            };
            store.save(&journal)?;
            let outcome = (|| -> Result<Value> {
                journal.engine_started = true;
                store.save(&journal)?;
                let mut prepared = candidate.clone();
                prepared["backup"] = json!(journal.engine_backup);
                let engine = duck_play_integrity::request(ctx, "apply", &prepared)?;
                if engine["backup"] != journal.engine_backup {
                    bail!("engine backup changed during apply");
                }
                let mut restart = engine["restart_required"] == true;
                if duck_play_integrity::request(ctx, "power", &json!({}))?["stop_revision"]
                    != stop_revision
                {
                    bail!("engine was stopped during configuration application");
                }
                if let Some(backup) = &journal.patch_backup {
                    journal.patch_started = true;
                    store.save(&journal)?;
                    restart |= patch_commit(ctx, backup)?["restart_required"] == true;
                }
                if duck_play_integrity::request(ctx, "power", &json!({}))?["stop_revision"]
                    != stop_revision
                {
                    bail!("engine was stopped during configuration application");
                }
                journal.restart_required = restart;
                journal.phase = "completed".into();
                store.save(&journal)?;
                Ok(
                    json!({"config":engine["config"],"backup":journal.engine_backup,"restart_required":restart}),
                )
            })();
            match outcome {
                Ok(value) => Ok(value),
                Err(error) => match recover(ctx, &store, &mut journal) {
                    Ok(_) => {
                        Err(error.context("application failed; original configurations restored"))
                    }
                    Err(recovery) => {
                        Err(error.context(format!("recovery incomplete: {recovery:#}")))
                    }
                },
            }
        }
    }
}
fn patch_prepare(ctx: &Context, value: &Value) -> Result<Value> {
    if value["target"] != "tricky-store" {
        bail!("unsupported patch participant");
    }
    duck_tricky_store::patch::prepare(ctx, &serde_json::from_value(value.clone())?)
}
fn patch_commit(ctx: &Context, token: &str) -> Result<Value> {
    duck_tricky_store::patch::commit_prepared(ctx, token)
}
fn patch_restore(ctx: &Context, token: &str) -> Result<Value> {
    duck_tricky_store::patch::restore(
        ctx,
        &duck_tricky_store::patch::Request {
            backup: token.into(),
            ..Default::default()
        },
    )
}
