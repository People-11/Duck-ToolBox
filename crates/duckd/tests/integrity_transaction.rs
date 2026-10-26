#![cfg(unix)]
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn process_termination_leaves_a_confirmable_recovery_record() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("play-integrity")).unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    fs::write(root.path().join("module.prop"), "id=duck-toolbox\n").unwrap();
    let engine = root.path().join("play-integrity/pif-engine");
    fs::write(
        &engine,
        r#"#!/bin/sh
base=$(dirname "$0")
cat >/dev/null
case "$2" in
 prepare) printf '{"ok":true,"api":1,"data":{"backup":"fixture-backup"}}' ;;
 apply)
   printf changed > "$base/state"
   touch "$base/blocked"
   while [ -f "$base/blocked" ]; do sleep 0.01; done
   printf '{"ok":true,"api":1,"data":{"backup":"fixture-backup"}}'
   ;;
 restore)
   rm -f "$base/state"
   printf '{"ok":true,"api":1,"data":{"restart_required":true}}'
   ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(&engine, fs::Permissions::from_mode(0o700)).unwrap();
    let command = || {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_duckd"));
        cmd.env("DUCK_TOOLBOX_ROOT", root.path())
            .env("DUCK_TOOLBOX_DATA_ROOT", root.path().join("data"))
            .env("DUCK_TOOLBOX_SYSROOT", root.path().join("sys"));
        cmd
    };
    let mut child = command()
        .args(["integrity-transaction", "apply", "--stdin-json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"engine":{"content":"MODEL=A","revision":"fixture"}}"#)
        .unwrap();
    let blocked = root.path().join("play-integrity/blocked");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !blocked.exists() {
        assert!(Instant::now() < deadline, "apply did not reach publication");
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    fs::remove_file(blocked).unwrap();
    let journal_path = root
        .path()
        .join("data/var/integrity-transaction/journal.json");
    let journal: Value = serde_json::from_str(&fs::read_to_string(&journal_path).unwrap()).unwrap();
    assert_eq!(journal["phase"], "applying");
    assert_eq!(journal["engine_started"], true);
    let status: Value = serde_json::from_slice(
        &command()
            .args(["integrity-transaction", "status"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(status["data"]["pending"], true);
    let mut recover = command()
        .args(["integrity-transaction", "recover", "--stdin-json"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(recover.stdin.take().unwrap(), &json!({"confirmed":true})).unwrap();
    let output = recover.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(!root.path().join("play-integrity/state").exists());
    let journal: Value = serde_json::from_str(&fs::read_to_string(journal_path).unwrap()).unwrap();
    assert_eq!(journal["phase"], "restored");
}
