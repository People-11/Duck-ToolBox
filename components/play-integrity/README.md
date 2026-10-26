# Play Integrity Engine

An independently built GPLv3 derivative of Play Integrity Fork, fixed to revision
`23ec56c2440c4fd1a5e893a23ef7861d6366a828`. See NOTICE, LICENSE and UPSTREAM.json.
This copy is modified (2026-10-26) to run with fixed default options only.

## Build and installation

Requires Python 3.12+, Rust 1.89+, JDK 21, Android SDK 37, Build Tools 37.0.0 and NDK
30.0.16248370. From the containing repository run
`python3 scripts/build-play-integrity.py`. The two Zygisk libraries, classes.dex,
scripts and arm64 CLI form the engine payload. The host CLI and tests build with
`cargo test --manifest-path Cargo.toml` here.

Root managers load `zygisk/arm64-v8a.so` and `zygisk/armeabi-v7a.so`. By default
the payload directory is `/data/adb/modules/duck-toolbox/play-integrity`; the data
directory is `/data/adb/play-integrity`. The CLI accepts PIF_ENGINE_MODULE,
PIF_ENGINE_DATA and PIF_ENGINE_SYSROOT; all must be absolute paths.

Call `pif-engine stage setup`, `stage early`, `stage late` and `stage uninstall`
from the corresponding root-manager lifecycle. The late stage must run after
boot completion. No network request occurs at boot.

## Fixed options

The options upstream exposes are fixed to their defaults: full mode (injection plus
compatibility scripts; there is no scripts-only mode), property deletion and persistent
compatibility properties enabled, no device matching during generation, strong
generation, and the bundled `scripts/app_replace_list.txt`. The engine runs whenever the
containing module is enabled, no conflicting PIF/USNF module is enabled, and a nonempty
configuration is saved. The root manager's disable/remove flags are the only stop control.

## Process interface (version 1)

`pif-engine request OPERATION` reads optional JSON on stdin and prints one object:
`{ "ok": true, "api": 1, "data": ... }`, or an error with code/message/details.
Exit status is nonzero on failure.

| Operation | Input / result |
| --- | --- |
| status | Prerequisites, conflicts, boot evidence and reboot requirement |
| config | Canonical prop/JSON content, entries and revision |
| preview | content, format (auto/prop/json); returns changes, revision and suggested security patch |
| prepare | Preview content plus revision; durably saves a backup token before publication |
| apply | Preview content plus revision and optional prepared backup token; atomic publication and backup token |
| restore | backup token; refuses to overwrite a newer configuration |
| task-start | Starts Pixel Canary fingerprint generation (strong, at most 120 seconds) |
| task-status | id; returns state, bounded output and candidate preview |
| restart | Restarts only DroidGuard and Play Store; reports each process result |

Fingerprint tasks run in private directories and never install a candidate.
Completed tasks retain their result and a bounded log; downloaded working files are
removed. HTTPS (HTTP/1.1) uses certificate validation and a bounded response body; independent
pages are downloaded concurrently. A task's
estimated expiry is upstream's heuristic, not proof of continued server acceptance.

ROM replacement paths require reboot. Only a single application/overlay entry under
`/{system,product,system_ext,vendor,odm,oem}/{app,priv-app,overlay}/NAME` is accepted.
The CLI and boot scripts both validate the list before creating any overlay artifacts.

DenyList changes are journaled per package/process before each mutation and read back;
the uninstall stage restores recorded values only while the package still matches the
expected state. Permissive SELinux visibility permissions are recorded and restored the
same way; the enforcing state is never changed. Persistent compatibility properties are
restored by writing their recorded original values.

No build or local test proves Play Integrity acceptance; hardware, ROM, Zygisk,
Google Play Services and current server policy require real-device verification.
See [RULES.md](RULES.md) for the pinned upstream sources and behavioral limits.
