// SPDX-License-Identifier: GPL-3.0-only
// Independent process tests for snapshots, manual observations and read-only comparison.
use serde_json::{Value, json};
use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

fn request(root: &Path, operation: &str, input: Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_pif-engine"))
        .args(["request", operation])
        .env("PIF_ENGINE_DATA", root.join("data"))
        .env("PIF_ENGINE_SYSROOT", root.join("system"))
        .env("PIF_ENGINE_MODULE", root.join("module"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap()
}
#[test]
fn snapshots_remain_independent_of_applied_config_and_power() {
    let root = tempfile::tempdir().unwrap();
    request(root.path(), "disable", json!({}));
    let saved = request(
        root.path(),
        "profile-save",
        json!({"name":"自定义","content":"MODEL=Candidate\ncustom.flag=keep\nspoofProvider=1"}),
    );
    assert_eq!(saved["ok"], true, "{saved}");
    let id = &saved["data"]["profiles"][0]["id"];
    assert!(!root.path().join("data/custom.pif.json").exists());
    assert_eq!(
        request(root.path(), "power", json!({}))["data"]["enabled"],
        false
    );
    let preview = request(root.path(), "profile-preview", json!({"id":id}));
    assert_eq!(preview["data"]["entries"]["custom.flag"], "keep");
    assert_eq!(preview["data"]["entries"]["spoofProvider"], "1");
    assert_eq!(preview["data"]["changes"][0]["before"], Value::Null);
    let before = request(root.path(), "config", json!({}));
    request(
        root.path(),
        "apply",
        json!({"content":"MODEL=Other","revision":before["data"]["revision"]}),
    );
    let fresh = request(root.path(), "profile-preview", json!({"id":id}));
    assert_eq!(
        fresh["data"]["revision"],
        request(root.path(), "config", json!({}))["data"]["revision"]
    );
    assert_ne!(fresh["data"]["revision"], preview["data"]["revision"]);
    assert_eq!(
        request(root.path(), "profile-delete", json!({"id":"../custom.pif"}))["ok"],
        false
    );
    assert_eq!(
        request(root.path(), "profile-delete", json!({"id":id}))["data"]["profiles"],
        json!([])
    );
    assert_eq!(
        request(root.path(), "config", json!({}))["data"]["entries"]["MODEL"],
        "Other"
    );
}
#[test]
fn profile_inputs_and_capacity_are_bounded_without_overwriting() {
    let root = tempfile::tempdir().unwrap();
    for input in [
        json!({"name":"","content":"MODEL=A"}),
        json!({"name":"bad\nname","content":"MODEL=A"}),
        json!({"name":"x","content":"invalid"}),
        json!({"name":12,"content":"MODEL=A"}),
    ] {
        assert_eq!(request(root.path(), "profile-save", input)["ok"], false);
    }
    for i in 0..12 {
        assert_eq!(
            request(
                root.path(),
                "profile-save",
                json!({"name":format!("Snapshot {i}"),"content":"MODEL=A"})
            )["ok"],
            true
        );
    }
    assert_eq!(
        request(
            root.path(),
            "profile-save",
            json!({"name":"Overflow","content":"MODEL=B"})
        )["ok"],
        false
    );
    assert_eq!(
        request(root.path(), "profile-list", json!({}))["data"]["profiles"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
}
#[test]
fn manual_tests_keep_revision_and_mode_and_never_claim_verification() {
    let root = tempfile::tempdir().unwrap();
    let config = request(root.path(), "config", json!({}))["data"].clone();
    let mut record = json!({"app":"Play Store","version":"1","note":"manual","basic":"pass","device":"unknown","strong":"fail","revision":config["revision"]});
    record["revision"] = json!("stale");
    assert_eq!(
        request(root.path(), "record-save", record.clone())["ok"],
        false
    );
    record["revision"] = config["revision"].clone();
    record["strong"] = json!("guaranteed");
    assert_eq!(
        request(root.path(), "record-save", record.clone())["ok"],
        false
    );
    record["strong"] = json!("fail");
    request(root.path(), "disable", json!({}));
    for i in 0..26 {
        record["version"] = json!(i.to_string());
        assert_eq!(
            request(root.path(), "record-save", record.clone())["ok"],
            true
        );
    }
    let saved = request(root.path(), "record-list", json!({}))["data"].clone();
    assert_eq!(saved["user_reported"], true);
    assert_eq!(saved["verdict_verified"], false);
    assert_eq!(saved["records"].as_array().unwrap().len(), 24);
    assert_eq!(saved["records"][0]["version"], "25");
    assert_eq!(saved["records"][0]["enabled"], false);
    assert_eq!(saved["records"][0]["revision"], config["revision"]);
    assert_eq!(
        request(root.path(), "record-clear", json!({}))["data"]["records"],
        json!([])
    );
    assert_eq!(request(root.path(), "config", json!({}))["data"], config);
}

#[test]
fn a_corrupt_snapshot_does_not_hide_valid_snapshots_or_evade_capacity() {
    let root = tempfile::tempdir().unwrap();
    let first = request(
        root.path(),
        "profile-save",
        json!({"name":"Corrupt","content":"MODEL=A"}),
    );
    let id = first["data"]["profiles"][0]["id"].as_str().unwrap();
    let path = root.path().join("data/profiles").join(format!("{id}.json"));
    std::fs::write(&path, "{broken").unwrap();
    for i in 0..11 {
        assert_eq!(
            request(
                root.path(),
                "profile-save",
                json!({"name":format!("Valid {i}"),"content":"MODEL=B"})
            )["ok"],
            true
        );
    }
    let listed = request(root.path(), "profile-list", json!({}));
    assert_eq!(listed["data"]["profiles"].as_array().unwrap().len(), 11);
    assert_eq!(listed["data"]["invalid_profiles"][0]["id"], id);
    assert_eq!(
        request(root.path(), "profile-preview", json!({"id":id}))["ok"],
        false
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), "{broken");
    assert_eq!(
        request(
            root.path(),
            "profile-save",
            json!({"name":"Overflow","content":"MODEL=C"})
        )["ok"],
        false
    );
    assert_eq!(
        request(root.path(), "profile-delete", json!({"id":id}))["ok"],
        true
    );
    assert_eq!(
        request(root.path(), "profile-list", json!({}))["data"]["invalid_profiles"],
        json!([])
    );
}
