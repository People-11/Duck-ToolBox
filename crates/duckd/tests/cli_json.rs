//! End-to-end tests that run the real `duckd` binary and assert on its JSON envelopes.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use serde_json::Value;

struct Env {
    root: PathBuf,
    data: PathBuf,
    sysroot: PathBuf,
}

fn env(name: &str) -> Env {
    let base = std::env::temp_dir().join(format!(
        "duckd-cli-{name}-{}-{}",
        std::process::id(),
        nanos()
    ));
    let root = base.join("module");
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(
        root.join("module.prop"),
        "id=duck-toolbox\nname=Duck ToolBox\n",
    )
    .unwrap();
    let env = Env {
        data: base.join("data"),
        sysroot: base.join("sys"),
        root,
    };
    fs::create_dir_all(&env.sysroot).unwrap();
    env
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos()
}

fn command(env: &Env) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_duckd"));
    command
        .env("DUCK_TOOLBOX_ROOT", &env.root)
        .env("DUCK_TOOLBOX_DATA_ROOT", &env.data)
        .env("DUCK_TOOLBOX_SYSROOT", &env.sysroot);
    command
}

fn run(env: &Env, args: &[&str]) -> Value {
    let output = command(env).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "command {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn run_json(env: &Env, args: &[&str], stdin: &Value) -> std::process::Output {
    let mut child = command(env)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    serde_json::to_writer(child.stdin.take().unwrap(), stdin).unwrap();
    child.wait_with_output().unwrap()
}

fn install_backend(env: &Env, id: &str, version_code: u64) {
    let dir = env.sysroot.join(format!("data/adb/modules/{id}"));
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("module.prop"),
        format!("id={id}\nname={id}\nversionCode={version_code}\n"),
    )
    .unwrap();
}

#[test]
fn features_manifest_lists_every_registered_tool() {
    let env = env("features");
    let payload = run(&env, &["features"]);
    assert_eq!(payload["command"], "features");
    let ids: Vec<&str> = payload["data"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| feature["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["rkp-device", "tricky-store"]);
    assert_eq!(payload["data"]["api"], 1);
    let device = payload["data"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["id"] == "rkp-device")
        .unwrap();
    assert_eq!(device["contract"], 5);
}

#[test]
fn explicit_device_rkp_protocol_reports_only_its_own_resource_requirements() {
    let environment = env("device-rkp-protocol-prerequisites");
    for (protocol, required, unrelated) in [
        (
            "v1",
            "v1_unavailable",
            vec!["keymint_library_missing", "smcinvoke_missing"],
        ),
        ("v2", "v2_unavailable", vec!["v1_unavailable"]),
        (
            "v1-native",
            "v1_native_unavailable",
            vec!["v1_unavailable", "v2_unavailable"],
        ),
        ("auto", "v1_unavailable", vec![]),
    ] {
        let output = command(&environment)
            .args([
                "rkp-device",
                "provision",
                "--acknowledge-limitations",
                "--protocol",
                protocol,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(payload["error"]["code"], "unsupported_device");
        let message = payload["error"]["message"].as_str().unwrap();
        assert!(message.contains(required), "{message}");
        if !cfg!(all(target_os = "android", target_arch = "aarch64")) {
            assert!(
                message.contains("android_arm64_required"),
                "shared gate must remain: {message}"
            );
        }
        for blocker in unrelated {
            assert!(
                !message.contains(blocker),
                "unrelated protocol requirement: {message}"
            );
        }
        if protocol == "auto" {
            assert!(message.contains("keymint_library_missing"));
            assert!(message.contains("smcinvoke_missing"));
        }
    }
}

#[test]
fn usage_errors_are_json_envelopes() {
    let env = env("usage");
    let output = command(&env)
        .args(["tricky-store", "no-such-command"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert_eq!(payload["error"]["code"], "usage_error");
}

#[test]
fn tricky_store_status_without_backend() {
    let env = env("ts-none");
    let payload = run(&env, &["tricky-store", "status", "--json"]);
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["command"], "tricky-store.status");
    assert!(payload["data"]["active"].is_null());
    assert_eq!(payload["data"]["backends"].as_array().unwrap().len(), 0);
}

#[test]
fn tricky_store_keybox_install_rejects_bad_xml() {
    let env = env("ts-bad-keybox");
    install_backend(&env, "tricky_store", 246);
    let source = env.sysroot.join("storage/emulated/0/Download/bad.xml");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "<not-keybox />").unwrap();

    let output = command(&env)
        .args([
            "tricky-store",
            "keybox",
            "install",
            "/storage/emulated/0/Download/bad.xml",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let payload: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(payload["error"]["code"], "invalid_keybox");
}

#[test]
fn tricky_store_set_aosp_installs_bundled_keybox() {
    let env = env("ts-aosp");
    install_backend(&env, "tricky_store", 246);

    let payload = run(&env, &["tricky-store", "keybox", "set-aosp", "--json"]);
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["data"]["summary"]["has_ecdsa"], true);
    assert_eq!(payload["data"]["summary"]["has_rsa"], true);

    let installed = env.sysroot.join("data/adb/tricky_store/keybox.xml");
    assert!(Path::new(&installed).is_file());
}

#[test]
fn tricky_store_keybox_import_accepts_picked_content() {
    let env = env("ts-import");
    install_backend(&env, "tricky_store", 246);
    let aosp = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../duck-tricky-store/assets/aosp-keybox.xml"),
    )
    .unwrap();

    let output = run_json(
        &env,
        &["tricky-store", "keybox", "import", "--stdin-json"],
        &serde_json::json!({ "content": aosp }),
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        env.sysroot
            .join("data/adb/tricky_store/keybox.xml")
            .is_file()
    );
}

fn description(env: &Env) -> String {
    fs::read_to_string(env.root.join("module.prop"))
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("description="))
        .unwrap()
        .to_owned()
}

#[test]
fn renew_without_backend_only_reports() {
    let env = env("renew-none");
    let payload = run(&env, &["renew"]);
    assert_eq!(payload["data"]["renewed"], false);
    assert!(payload["data"]["error"].is_null());
    assert!(
        description(&env).contains("no Tricky Store"),
        "{}",
        description(&env)
    );
    assert!(!env.data.join("var/renew.json").exists());
}

#[test]
fn renew_leaves_a_valid_keybox_alone() {
    let env = env("renew-valid");
    install_backend(&env, "tricky_store", 246);
    run(&env, &["tricky-store", "keybox", "generate"]);

    let payload = run(&env, &["renew"]);
    assert_eq!(payload["data"]["renewed"], false);
    assert!(payload["data"]["expires_at"].as_u64().is_some());
    let text = description(&env);
    assert!(text.starts_with("✅ Keybox valid until"), "{text}");
    assert!(text.ends_with("| tricky_store (TS): 0 targets"), "{text}");
    assert!(!env.data.join("var/renew.json").exists());
}

#[test]
fn renew_records_a_failed_issuance_and_waits_before_retrying() {
    let env = env("renew-missing");
    install_backend(&env, "tricky_store", 246);

    // The host is not a Qualcomm Android device, so issuance fails before any request.
    let payload = run(&env, &["renew"]);
    assert_eq!(payload["data"]["renewed"], false);
    let error = payload["data"]["error"].as_str().unwrap();
    assert!(error.starts_with("unsupported_device: "), "{error}");
    assert!(
        description(&env).starts_with("❌ No usable keybox; renewal failed (unsupported_device")
    );

    let state = fs::read_to_string(env.data.join("var/renew.json")).unwrap();
    run(&env, &["renew"]);
    assert_eq!(
        fs::read_to_string(env.data.join("var/renew.json")).unwrap(),
        state
    );

    let forced = command(&env).args(["renew", "--force"]).output().unwrap();
    assert!(!forced.status.success());
    let payload: Value = serde_json::from_slice(&forced.stdout).unwrap();
    assert_eq!(payload["ok"], false);
    assert!(
        payload["error"]["message"]
            .as_str()
            .unwrap()
            .contains("unsupported_device")
    );
}
