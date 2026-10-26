// SPDX-License-Identifier: GPL-3.0-only
// Independent CLI integration tests; no toolbox executable or runtime is involved.
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn request(root: &std::path::Path, op: &str, input: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pif-engine"))
        .args(["request", op])
        .env("PIF_ENGINE_DATA", root.join("data"))
        .env("PIF_ENGINE_MODULE", root.join("module"))
        .env("PIF_ENGINE_SYSROOT", root.join("sys"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(serde_json::to_string(&input).unwrap().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    serde_json::from_slice(&output.stdout).unwrap()
}
fn fixture(script: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("module/scripts")).unwrap();
    fs::write(root.path().join("module/scripts/autopif4.sh"), script).unwrap();
    root
}
fn terminal(root: &std::path::Path, id: &Value) -> Value {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let result = request(root, "task-status", json!({"id":id}));
        if result["data"]["state"] != "running" {
            return result;
        }
        assert!(Instant::now() < end, "worker failed to terminate");
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
fn generation_previews_without_publishing() {
    let root = fixture(
        "echo 'fetching fixture'; printf 'MODEL=Fixture\nSECURITY_PATCH=2026-10-05\n' > custom.pif.prop\n",
    );
    let started = request(root.path(), "task-start", json!({}));
    assert_eq!(started["ok"], true, "{started}");
    let result = terminal(root.path(), &started["data"]["id"]);
    assert_eq!(result["data"]["state"], "completed", "{result}");
    assert_eq!(result["data"]["result"]["entries"]["MODEL"], "Fixture");
    assert_eq!(result["data"]["result"]["suggested_patch"], "2026-10-05");
    assert!(!root.path().join("data/custom.pif.json").exists());
}
#[cfg(unix)]
#[test]
fn every_job_exit_stops_descendants_that_ignore_termination() {
    struct Cleanup(Vec<i32>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for pid in &self.0 {
                unsafe {
                    libc::kill(-pid, libc::SIGKILL);
                }
            }
        }
    }
    for scenario in ["overflow", "complete"] {
        let ending = match scenario {
            "overflow" => "head -c 1200000 /dev/zero; sleep 30",
            _ => "printf 'MODEL=Fixture\\n' > custom.pif.prop",
        };
        let root = fixture(&format!(
            r#"
echo $$ > "$PIF_ENGINE_DATA/generator.pid"
(trap '' TERM; while :; do printf x >> "$PIF_ENGINE_DATA/heartbeat"; sleep 0.02; done) &
while [ ! -s "$PIF_ENGINE_DATA/heartbeat" ]; do sleep 0.01; done
{ending}
"#
        ));
        let started = request(root.path(), "task-start", json!({}));
        assert_eq!(started["ok"], true, "{started}");
        let mut cleanup = Cleanup(vec![started["data"]["pid"].as_u64().unwrap() as i32]);
        let heartbeat = root.path().join("data/heartbeat");
        let end = Instant::now() + Duration::from_secs(5);
        while fs::metadata(&heartbeat).is_err() {
            assert!(Instant::now() < end, "generator did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        cleanup.0.push(
            fs::read_to_string(root.path().join("data/generator.pid"))
                .unwrap()
                .trim()
                .parse()
                .unwrap(),
        );
        let finished = terminal(root.path(), &started["data"]["id"]);
        let expected = if scenario == "complete" {
            "completed"
        } else {
            "failed"
        };
        assert_eq!(
            finished["data"]["state"], expected,
            "{scenario}: {finished}"
        );
        let before = fs::metadata(&heartbeat).unwrap().len();
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(
            fs::metadata(&heartbeat).unwrap().len(),
            before,
            "{scenario}: background writer survived job completion"
        );
        assert!(!root.path().join("data/custom.pif.json").exists());
    }
}
#[test]
fn conflicts_module_flags_and_stale_apply_are_explicit() {
    let root = fixture("exit 1\n");
    let conflict = root.path().join("sys/data/adb/modules/playintegrityfix");
    fs::create_dir_all(&conflict).unwrap();
    fs::write(conflict.join("module.prop"), "id=playintegrityfix").unwrap();
    assert_eq!(
        request(root.path(), "status", json!({}))["data"]["state"],
        "conflict"
    );
    fs::write(conflict.join("disable"), "").unwrap();
    let preview = request(root.path(), "preview", json!({"content":"MODEL=A"}));
    let applied = request(
        root.path(),
        "apply",
        json!({"content":"MODEL=A","revision":preview["data"]["revision"]}),
    );
    assert_eq!(applied["ok"], true);
    assert_eq!(
        request(
            root.path(),
            "apply",
            json!({"content":"MODEL=B","revision":preview["data"]["revision"]})
        )["ok"],
        false
    );
    fs::write(root.path().join("disable"), "").unwrap();
    assert_eq!(
        request(root.path(), "status", json!({}))["data"]["state"],
        "disabled"
    );
    for removed in ["options", "disable", "power", "task-cancel", "profile-list"] {
        assert_eq!(request(root.path(), removed, json!({}))["ok"], false);
    }
}
#[test]
fn insecure_fetch_is_rejected_without_network() {
    let output = Command::new(env!("CARGO_BIN_EXE_pif-engine"))
        .args(["fetch", "-O", "/tmp/not-created", "http://localhost/"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only HTTPS"));
}

#[test]
fn magisk_directory_is_not_zygisk_runtime_evidence() {
    let root = fixture("exit 0\n");
    fs::create_dir_all(root.path().join("sys/data/adb/magisk")).unwrap();
    let result = request(root.path(), "status", json!({}));
    assert_eq!(result["data"]["zygisk_detected"], false);
    let provider = root.path().join("sys/data/adb/modules/zygisksu");
    fs::create_dir_all(&provider).unwrap();
    fs::write(provider.join("module.prop"), "id=zygisksu").unwrap();
    assert_eq!(
        request(root.path(), "status", json!({}))["data"]["zygisk_detected"],
        true
    );
    fs::write(provider.join("disable"), "").unwrap();
    assert_eq!(
        request(root.path(), "status", json!({}))["data"]["zygisk_detected"],
        false
    );
}

#[test]
fn lifecycle_uses_cleanup_when_disabled_and_records_boot_state() {
    let root = fixture("exit 0\n");
    fs::write(
        root.path().join("module/scripts/stage.sh"),
        "printf '%s\\n' \"$1\" >> \"$PIF_ENGINE_DATA/stage.trace\"\n",
    )
    .unwrap();
    fs::write(root.path().join("disable"), "").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_pif-engine"))
        .args(["stage", "early"])
        .env("PIF_ENGINE_DATA", root.path().join("data"))
        .env("PIF_ENGINE_MODULE", root.path().join("module"))
        .env("PIF_ENGINE_SYSROOT", root.path().join("sys"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("data/stage.trace")).unwrap(),
        "cleanup\n"
    );
    let evidence: Value =
        serde_json::from_slice(&fs::read(root.path().join("data/boot.json")).unwrap()).unwrap();
    assert_eq!(evidence["active"], false);
}

#[cfg(unix)]
#[test]
fn magisk_adapter_journals_cli_changes_and_retains_external_edits() {
    use std::os::unix::fs::PermissionsExt;
    let root = fixture("exit 0\n");
    fs::write(root.path().join("module/scripts/stage.sh"), "exit 0\n").unwrap();
    let magisk_dir = root.path().join("sys/data/adb/magisk");
    fs::create_dir_all(&magisk_dir).unwrap();
    let magisk = magisk_dir.join("magisk");
    fs::write(&magisk,r#"#!/bin/sh
base=${0%/*}
case "$2" in
status) exit 0 ;;
ls) cat "$base/list" ;;
rm) awk -F'|' -v pkg="$3" -v proc="$4" '$1!=pkg || $2!=proc' "$base/list" > "$base/next"; mv "$base/next" "$base/list" ;;
add) printf '%s|%s\n' "$3" "$4" >> "$base/list" ;;
esac
"#).unwrap();
    fs::set_permissions(&magisk, fs::Permissions::from_mode(0o700)).unwrap();
    let original =
        "com.google.android.gms|com.google.android.gms\ncom.android.vending|com.android.vending\n";
    fs::write(magisk_dir.join("list"), original).unwrap();
    let preview = request(root.path(), "preview", json!({"content":"MODEL=A"}));
    assert_eq!(
        request(
            root.path(),
            "apply",
            json!({"content":"MODEL=A","revision":preview["data"]["revision"]})
        )["ok"],
        true
    );
    let stage = |phase: &str| {
        Command::new(env!("CARGO_BIN_EXE_pif-engine"))
            .args(["stage", phase])
            .env("PIF_ENGINE_DATA", root.path().join("data"))
            .env("PIF_ENGINE_MODULE", root.path().join("module"))
            .env("PIF_ENGINE_SYSROOT", root.path().join("sys"))
            .output()
            .unwrap()
    };
    assert!(stage("early").status.success());
    assert_eq!(fs::read_to_string(magisk_dir.join("list")).unwrap(), "");
    assert!(stage("uninstall").status.success());
    let entries = |s: &str| {
        s.lines()
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(
        entries(&fs::read_to_string(magisk_dir.join("list")).unwrap()),
        entries(original)
    );
    assert!(stage("early").status.success());
    fs::write(
        magisk_dir.join("list"),
        "com.google.android.gms|com.google.android.gms.external\n",
    )
    .unwrap();
    assert!(!stage("uninstall").status.success());
    let saved = fs::read_to_string(magisk_dir.join("list")).unwrap();
    assert!(saved.contains(".external"));
    assert!(!saved.contains("com.google.android.gms|com.google.android.gms\n"));
    // Resolve only the GMS conflict; Play Store was already restored independently.
    fs::write(
        magisk_dir.join("list"),
        "com.android.vending|com.android.vending\n",
    )
    .unwrap();
    assert!(stage("uninstall").status.success());
    assert_eq!(
        entries(&fs::read_to_string(magisk_dir.join("list")).unwrap()),
        entries(original)
    );
}
