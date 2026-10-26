# Duck ToolBox

Duck ToolBox is a root module (KernelSU, APatch or Magisk) with three jobs, and no WebUI:

- keep the target list of the active keystore module (Tricky Store, TEESimulator or
  OhMyKeymint) in step with the installed apps;
- keep that module's keybox valid with RKP keyboxes issued by this device's own
  KeyMint TA (Qualcomm, or MediaTek through AIDL V1), renewing each one a day before it
  expires;
- after each issuance, generate a Play Integrity fingerprint, apply it with a matching
  keystore security patch and restart the Play Integrity processes.

Its state is shown as the module description in the root manager, for example
`✅ Keybox valid until 2026-10-30 05:26 UTC, renews after 2026-10-29 05:26 UTC | Tricky Store (TS): 23 targets`.
The action button issues and installs a keybox immediately.

## Target list

Once at boot, then whenever the package list changes (`tricky-store auto-apply`):

- user apps installed since the previous run are added; the first run adds every user
  app;
- the necessary system apps are added when installed (Google Play services, the Play
  Store, and the OPPO/ColorOS services Tricky Addon lists);
- unnecessary apps are removed: Tricky Addon's list of root managers and tools (bundled,
  refreshed from upstream at boot) and every installed Xposed module.

Package changes are detected with BusyBox `inotifyd` on `/data/system/packages.list`,
which PackageManager rewrites on every install, update and removal. A burst of changes (a
restore, a batch of updates) is handled once, 10 seconds after the last change. Without
`inotifyd`, or if it stops, the list is checked every 10 minutes instead.

Apps you remove from the list stay removed unless they are necessary apps. Modes, policies
and every other part of the keystore module's configuration are left as they are. Supported
layouts: Tricky Store `config.ini` and legacy `target.txt`, TEESimulator `config.json`, and
OhMyKeymint `injector.toml`.

## Play Integrity

A bundled, independently built GPLv3 engine derived from Play Integrity Fork (Zygisk
libraries, DroidGuard/Play Store injection and its sensitive-property scripts) lives in
`components/play-integrity`; see its [README](components/play-integrity/README.md) and
[licensing boundaries](THIRD_PARTY_NOTICES.md). All of its options are fixed to the
defaults: full mode only (no scripts-only mode), no property skips, strong generation
without device matching, and the bundled ROM app replacement list. Injection requires
Zygisk and a saved fingerprint; it stops when this module is disabled or when another
Play Integrity Fork/USNF module is enabled.

The fingerprint is refreshed only after a keybox is issued, and also once while the engine
has no fingerprint at all; nothing checks for updates otherwise. A refresh generates a
Pixel Canary fingerprint, applies it together with the active keystore module's security
patch policy (`SECURITY_PATCH` from the fingerprint, when it gives one date) as one
recoverable transaction, then restarts DroidGuard and the Play Store. A failed refresh is
retried at most hourly by the renewal checks until it succeeds.

## Keybox renewal

`duckd renew` reads the keybox of the active keystore module and takes the earliest
`notAfter` among its certificates. When that is less than a day away, already past, or no
certificate can be read (including no keybox at all), it issues a new keybox through the
device RKP issuer below and installs it in place of the old one (the previous file is kept
as a backup). The boot script runs it every 30 minutes; `sleep` pauses in deep sleep, so
checks follow the time the device is awake. Automatic attempts are limited to one per hour,
whatever the outcome; `duckd renew --force` (the action button) skips both checks.

Each run rewrites the `description=` line of `module.prop` with one of:

| State | Description starts with |
| --- | --- |
| Valid | `✅ Keybox valid until <expiry>, renews after <expiry - 1 day>` |
| Due, not yet attempted | `⏳ Keybox expires <expiry>; renewal pending` |
| Last attempt failed | `❌ Keybox expires <expiry>; renewal failed (<code>: <reason>), retry after <time>` |
| No readable keybox | `⏳` or `❌` followed by `No usable keybox; ...` |

then the Play Integrity state (`✅ PIF <model> (<patch>)`, `⏳ PIF fingerprint pending` or
`❌ PIF fingerprint failed (...)`), then the keystore module's state (`Tricky Store (TS): 23 targets`, or a notice when
no keystore module is installed). Installing this module counts as acknowledging the
issuer's limitations below: only some devices work, and only RKP certificates are issued.

### Device RKP issuer

- Asks this device's Qualcomm KeyMint TA for a `CSR_V2` over a P-256 key we generated,
  submits the result to Google's RKP server and exports `keybox.xml` with the private key.
- Supports a legacy V1 AIDL path for Qualcomm PureSoft and MediaTek firmware with a zero HBK.
  The public-key MAC and CSR are constructed in Rust; a small isolated bridge calls
  the device's own AIDL proxy, using the EEK curve reported by its HAL.
  This method is adapted from [LazyBonesLZY's rkp-v1-oracle](https://github.com/LazyBonesLZY/rkp-v1-oracle)
  at revision `b28480c98c1256753aed140c6ff86d0336f55c45` (MIT; see `module/licenses/rkp-v1-oracle.txt`).
  MediaTek uses only this AIDL adapter. Native V1 and V2 require Qualcomm identification
  as well as the vendor library and device node. Real-device
  compatibility and production server acceptance for MediaTek remain unverified.
- `rkp-device provision --protocol` selects `auto`, `v1`, `v2` or `v1-native`; `renew` uses `auto`.
  Automatic mode tries the available methods in order: V2, AIDL V1, then native V1.
  Each failed signing, CSR validation, server request or certificate validation moves
  to the next method with a fresh challenge. It stops at the first complete success;
  if every method fails, the error retains each method's name, code and reason.
  An explicit selection tries only that method. Files are written only after issuance
  and certificate validation succeed; a local save failure does not repeat issuance.
- Native V1 uses legacy vendor command `0x311e`, adapted from
  [aviraxp's native V1 contribution](https://github.com/eltavine/Duck-ToolBox/pull/19).
  It supplies a fresh MAC key, the public-key MAC and the complete selected EEK chain
  directly to the TA, reusing the isolated vendor worker. It needs the same library
  and device node as V2, and does not require Java or a system RKP APK at runtime.
  The vendor framing and EEK curve compatibility require real-device validation.
  It shares V1 CSR assembly and structural validation with the AIDL adapter,
  preserving opaque DeviceInfo and ProtectedData.
- Checks Android arm64, root and Qualcomm SoC before sending anything. V2 needs
  `/dev/smcinvoke` and `libkeymasterdeviceutils.so`; AIDL V1 needs `app_process` and a
  system RKP APK containing the matching AIDL classes. Neither check proves firmware support.
- V1 encrypts the DICE and signing evidence for the server. Its local report is
  explicitly structural only (`local_verify: null`); V2 retains local signature checks.
  Results include `csr_protocol` (`v1` / `v2`) and `csr_backend` (`aidl` / `native`).
  The `attempts` field records method failures and the final successful method.
- Starts its isolated worker with vendor library search paths while retaining the
  default namespace's system dependency links. Library loading and the firmware ABI
  still need to work on the device; prerequisite checks alone cannot confirm them.
- The exported keybox contains a software-generated private key. Output directories
  are private, and both the keybox and the private key are saved with mode `0600`.
- Only works on some devices, and only RKP certificates can be issued. `provision` refuses
  to run without `--acknowledge-limitations`; `renew` and the action button pass it.
- On a OnePlus PJX110 running Android 16, loading from `/data/adb` and calling the TA
  were verified. Both this implementation and the reference client received TA status
  `-18` for the software-key `CSR_V2` request; that test did not issue a keybox.
  The V1 adapter has host protocol/bridge tests and an Android build check; device
  compatibility and successful Google issuance still require a real-device run.

## Architecture

```txt
crates/
  duck-core/          Shared runtime: paths, JSON envelope, Feature trait, command log,
                      and a Sysroot so every feature is testable off-device.
  duck-platform/      Device adapters: props, pm, modules, root managers, SoC, HTTP.
  duck-rkp/           RKP protocol library: CBOR/COSE, CSR verification, RKP server client,
                      keybox XML.
  duck-rkp-device/    Device RKP issuer.
  duck-tricky-store/  Keystore module adapters, target list, keybox install and expiry,
                      security patch policy.
  duck-play-integrity/ MIT adapter for the engine's process interface.
  duckd/              The binary: `rkp-device`, `tricky-store`, `play-integrity` and
                      `integrity-transaction` subcommands, and `renew`, which joins them
                      and writes module.prop.
components/
  play-integrity/     The GPLv3 engine: CLI, Zygisk/Java injection and boot scripts.
xtask/                Repository checks (`cargo xtask line-limit`).
module/               Module payload: scripts, then the backend binary and engine.
```

Every command prints one JSON envelope: `{ ok, api, command, data, error, ts }`.

## Module lifecycle

| Stage | What runs |
| --- | --- |
| `customize.sh` | Checks the root manager, ABI and API level; prepares `/data/adb/duck-toolbox`; removes the WebUI/action links earlier releases put into the keystore module; runs the engine's `stage setup`; requests hot install from metamodules that support it. |
| `post-fs-data.sh` / `late-load.sh` | The engine's `stage early`: DenyList, ROM app replacement and early sensitive properties. KernelSU's late-load mode runs `late-load.sh` instead. |
| `service.sh` | Repairs the runtime directory. On Magisk, which has no boot-completed stage, it waits for boot and runs `boot-completed.sh`. |
| `boot-completed.sh` | Runs the engine's `stage late` (late sensitive properties), runs `tricky-store auto-apply` and starts its package-change watcher, starts the 30-minute `renew` loop, and sets the TSupport-Advance stop flag when that module is installed. |
| `action.sh` | `renew --force` (issuance, then a fingerprint refresh), prints the new description and keeps it on screen for 30 seconds. |
| `uninstall.sh` | Runs the engine's `stage uninstall` (restores DenyList, SELinux and persistent property changes), removes leftover entry links, the TSupport-Advance flag and `/data/adb/duck-toolbox`, including issued keyboxes and private keys. |

Saved data lives in `/data/adb/duck-toolbox/var`, outside the module directory, so updates
keep it: `renew.json` (last attempts and pending fingerprint), `tricky-store.toml` (apps
already seen), the configuration transaction journal, and each issuance's private run
directory under `outputs/`. The engine keeps its fingerprint in `/data/adb/play-integrity`.

**Updates.** Root managers follow `updateJson` in `module.prop`. CI uploads the module
directory as `duck-toolbox-<version>-<commit count>-canary`, and the commit count is its
`versionCode`. Each successful package on a `main` push or manual CI run also publishes the module ZIP
as a GitHub prerelease. Tags use `nightly-<8-character commit hash>-<UTC timestamp>`
(for example, `nightly-1234abcd-20261011T120000123456Z`). These releases do not replace
the latest stable release; Telegram uploads continue on push builds.
The manual **Stable** workflow reuses the CI checks and builds, preserves `version`
and `versionCode` from `module/module.prop`, publishes `v<version>` as the latest
non-prerelease release, and sends the same ZIP to Telegram with a `stable` label.
Before running it on `main`, increase `versionCode` beyond all accepted releases and
update `update.json` to match the version and `duck-toolbox-<version>.zip` asset.

**KernelSU Modules repository.** The root `module.yaml` supplies the module listing
metadata and selects `duck-toolbox-*.zip` release assets. Published ZIPs contain
`module.prop` at their root; the current builds target Android API 26+ on arm64-v8a.
The repository must enable release immutability and be registered through the
[KernelSU Modules developer portal](https://developers.kernelsu.org/).
Nightly prereleases are checked and recorded but are not displayed as stable versions.
Every new version must have a `versionCode` greater than all previously accepted
stable and prerelease versions; nightly CI uses the commit count. Rebuilding the same commit
creates another nightly release with the same `versionCode`, which is treated as the
same prerelease version. A stable release needs a higher `versionCode`, or an accepted
prerelease can be promoted by clearing its prerelease flag.

## JSON CLI

```txt
duckd features
duckd renew [--force]
duckd rkp-device status | provision [--protocol auto|v1|v2|v1-native] --acknowledge-limitations
duckd tricky-store status | save | auto-apply [--refresh] | patch preview|apply|restore
duckd play-integrity status | config | preview | apply | restore | task-start | task-status | restart
duckd integrity-transaction status | apply | recover
duckd tricky-store keybox install <file> | import | set-aosp | generate | fetch --url <u> [--decode <steps>]
duckd tricky-store keybox providers list | save | reset | import <file> | import-content | export [--path <file>]
duckd tricky-store apps xposed | denylist | unnecessary [--refresh]
```

Commands that take input read JSON from stdin with `--stdin-json`.

## Building and testing

Requirements:
- Rust stable with the `aarch64-linux-android` target (`rust-toolchain.toml` installs it).
- Android NDK r30.
- JDK 17 or newer and Android SDK Build Tools 36.0.0 for the embedded V1 bridge.
  Set `ANDROID_HOME` or `ANDROID_SDK_ROOT`; `ANDROID_BUILD_TOOLS_VERSION` can select
  another installed version. Set `JAVA_HOME` or put `java` and `javac` on `PATH`.
- For the Play Integrity engine: Python 3.12+, JDK 21, Android SDK 37 and Build Tools
  37.0.0 (see `components/play-integrity/README.md`).
- The device RKP issuer only builds for unix targets, so host tests need Linux or macOS.

```bash
cargo xtask line-limit
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
python3 scripts/test_rkp_v1_bridge.py  # AIDL fixture, framing and error propagation (JDK)
cargo test --manifest-path components/play-integrity/Cargo.toml
python3 scripts/test-play-integrity-scripts.py
python3 scripts/build-play-integrity.py  # engine payload into module/play-integrity and module/zygisk

# Device binary, e.g. with the NDK's clang as linker
cargo build --release --package duckd --target aarch64-linux-android
```

On Windows, `pwsh ./scripts/build.ps1 -PackageModule` builds the backend with `cargo-ndk`
and writes the module zip to `dist/`.

## Acknowledgements

- The Play Integrity engine derives from osm0sis, chiteroman and contributors'
  [Play Integrity Fork](https://github.com/osm0sis/PlayIntegrityFork) (GPLv3); its
  corresponding source is linked from the module's `licenses/play-integrity/SOURCE.txt`.

- The keystore module adapters, the unnecessary-apps list and the target list rules come
  from KOWX712's
  [Tricky Addon: Update Target List](https://github.com/KOWX712/Tricky-Addon-Update-Target-List)
  (Apache-2.0).
- The bundled software keybox is AOSP's reference attestation key
  (`system/keymaster/contexts/soft_attestation_cert.cpp`).
