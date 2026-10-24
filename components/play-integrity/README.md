# Play Integrity Engine

An independently built GPLv3 derivative of Play Integrity Fork, fixed to revision
`23ec56c2440c4fd1a5e893a23ef7861d6366a828`. See NOTICE, LICENSE and UPSTREAM.json.
It can run without the Duck ToolBox management executable or WebUI.

## Build and installation

Requires Python 3.12+, Rust 1.89+, JDK 21, Android SDK 37, Build Tools 37.0.0 and NDK
30.0.16248370. From the containing repository run
`python3 scripts/build-play-integrity.py`. The standalone source archive includes
the same build script, pinned native dependencies and engine source. The two
Zygisk libraries, classes.dex, scripts and arm64 CLI form the engine payload.
The host CLI and tests build with `cargo test --manifest-path Cargo.toml` here.

Root managers load `zygisk/arm64-v8a.so` and `zygisk/armeabi-v7a.so`. By default
the payload directory is `/data/adb/modules/duck-toolbox/play-integrity`; the data
directory is `/data/adb/play-integrity`. Native builds may override PIF_MODULE_DIR
and PIF_DATA_DIR for a standalone installation. The CLI accepts PIF_ENGINE_MODULE,
PIF_ENGINE_DATA and PIF_ENGINE_SYSROOT; all must be absolute paths.

Call `pif-engine stage setup`, `stage early`, `stage late` and `stage uninstall`
from the corresponding root-manager lifecycle. The late stage must run after
boot completion. No network request occurs at boot. Enabling is the default;
an absent or empty configuration keeps full-mode injection and global compatibility
scripts idle. Explicitly saving `mode=scripts-only` permits compatibility scripts
without a fingerprint. Enabling without a configuration also reserves the
sensitive-property handler, so the legacy handler does not apply overlapping
properties instead; the independent boot-hash setting retains its existing behavior.

## Public process interface (version 1)

`pif-engine request OPERATION` reads optional JSON on stdin and prints one object:
`{ "ok": true, "api": 1, "data": ... }`, or an error with code/message/details.
Exit status is nonzero on failure. The protocol does not link either executable
to the other and is usable from an ordinary root shell.

| Operation | Input / result |
| --- | --- |
| power / disable | Minimal power state / independent fail-closed stop; cancels generation, restores recorded compatibility changes, reports incomplete cleanup |
| status | Options, prerequisites, conflicts, boot evidence and reboot requirement |
| config | Canonical prop/JSON content, entries and revision |
| preview | content, format (auto/prop/json), optional migrate/advanced/force; returns changes and base revision |
| prepare | Preview content plus revision; durably saves a backup token before publication |
| apply | Preview content plus revision and optional prepared backup token; atomic publication and backup token |
| restore | backup token; refuses to overwrite a newer configuration |
| options | enabled, mode (full/scripts-only), skipdelprop, skippersistprop, skipmatchdevice, app_replace_list |
| task-start | strong (default true), match_device, optional timeout_seconds (1–120, default 120); starts fingerprint generation |
| task-status / task-cancel | id; returns state, bounded output and candidate preview |
| restart | Restarts only DroidGuard and Play Store; reports each process result |
| diagnose | Status and bounded relevant logcat output; no claimed integrity verdict |

Fingerprint tasks run in private directories, last at most 120 seconds and never
install a candidate or edit another module's security patch policy. Completed tasks
retain their result and a bounded log; downloaded working files are removed. HTTPS uses
certificate validation and a bounded response body. A task's estimated expiry
is upstream's heuristic, not proof of continued server acceptance.

An enabled conflicting PIF/USNF module prevents this engine from injecting or
running its property scripts. The engine does not uninstall other modules.
ROM replacement paths require reboot. Only a single application/overlay entry under
`/{system,product,system_ext,vendor,odm,oem}/{app,priv-app,overlay}/NAME` is accepted.
Wildcards are permitted only in application names; overlay wildcards and the
`overlay/config` directory are rejected. The CLI and boot scripts both validate
the list, including expanded names, before creating any overlay artifacts. Existing prop-handler and boot-hash
settings are respected by lifecycle integration. Changes to global properties
cannot be undone merely by unloading code from an already running process.

No build or local test proves Play Integrity acceptance; hardware, ROM, Zygisk,
Google Play Services and current server policy require real-device verification.

Configuration previews include non-mutating consistency warnings. New configurations
default Provider compatibility off; explicit legacy values remain intact. Ordinary
migration leaves missing launch/board API levels unset. Canary keeps upstream's
documented heuristics and labels them as estimates. See [RULES.md](RULES.md) for
the pinned Magisk, KernelSU and AOSP sources and the complete behavioral limits.

`pif-engine migrate-file INPUT OUTPUT --advanced --format json` provides standalone
safe migration, preserving unknown fields and backing up an existing output as
a `.bak` sibling. The shell wrapper delegates to this command.

`pif-engine request disable` remains usable with invalid configuration and while
another write holds the engine lock. A private stop token closes future companion
transfers and script mutations first. Explicit enabling acknowledges the current
token; a stale enable cannot acknowledge a later stop. Configuration is preserved,
and generation/import/application never enable the engine implicitly. Use
`request power` to inspect the gate and any retained cleanup warnings. `options`
with `enabled=false` also takes this shutdown path before validating other fields.

DenyList changes are journaled per package/process before each mutation and read
back. Shutdown restores recorded values only while the relevant package still
matches the expected state. External edits and unrecorded historical values are
preserved. Magisk may terminate processes when entries are added under enforcement.
Permissive SELinux visibility permissions are also recorded and conditionally
restored; the enforcing state is never changed. Persistent compatibility properties
retain upstream restoration by writing the recorded original values, which may
overwrite later edits to the same properties; the power control discloses this. Active process hooks require a
PI process restart, and mounts/global properties require a device reboot. Other
toolbox features remain independent. Boot shell stages are bounded to 30 seconds;
explicit cleanup has a five-second bound and retains recovery files on failure.

Configuration library and observations use the same version 1 request protocol:
`profile-list`, `profile-save` (`name`, `content`), `profile-preview` (`id`) and
`profile-delete` (`id`). At most 12 immutable snapshots are retained. Previewing
uses the current applied revision; saving, previewing or deleting a snapshot never
applies configuration or enables the engine.
Unreadable snapshots are reported individually, remain counted toward the storage
limit, and require explicit removal; they do not hide healthy snapshots.

`record-list`, `record-save` and `record-clear` manage the latest 24 manually
reported observations. A record supplies `app`, optional `version` and `note`,
`basic`/`device`/`strong` (`unknown`, `pass`, `fail`) and the saved configuration's
`revision`. Changed revisions are rejected. Records capture configuration identity
and engine mode at recording time and are explicitly marked unverified.

`inspect` compares saved Build/property overrides with current system property
readings using a bounded read-only command. Existing modules may already affect
those system readings. Target-process values remain unobserved unless measured;
the comparison does not establish injection or a Google Play Integrity verdict.
