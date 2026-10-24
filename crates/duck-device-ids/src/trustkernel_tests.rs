use super::*;
use crate::{
    kph_artifact,
    kph_process::{self, Runner},
    plan::{Plan, Request},
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    time::{Duration, Instant},
};

fn runner(
    root: &std::path::Path,
    fail_tag: &str,
    fail_verify: bool,
) -> (Runner, std::path::PathBuf) {
    let executable = root.join("probe.py");
    let log = root.join("calls.jsonl");
    let source = format!(
        r#"#!/usr/bin/env python3
import sys, pathlib, json, uuid
args = sys.argv[1:]
with open({log}, 'a') as log:
    entry = {{'args': args}}
    if args[1] == 'import_data':
        path = pathlib.Path(args[args.index('-i')+1])
        entry['hex'] = path.read_bytes().hex()
        entry['mode'] = path.stat().st_mode & 0o777
        assert args[args.index('-p')+1] == '42'
        assert args[args.index('-f')+1] == '0'
        value = args[args.index('-u')+1]
        assert str(uuid.UUID(value)) == value
    log.write(json.dumps(entry)+'\n')
if args[1] == 'ta_version': print('KPH TA VERSION=3.0')
elif args[1] == 'import_data' and args[args.index('-g')+1] == {fail_tag}:
    print('TEEC 0xffff0005: private diagnostic', file=sys.stderr)
    sys.exit(7)
elif args[1] == 'verify_data' and {fail_verify}: sys.exit(8)
"#,
        log = serde_json::to_string(&log.display().to_string()).unwrap(),
        fail_tag = serde_json::to_string(fail_tag).unwrap(),
        fail_verify = if fail_verify { "True" } else { "False" }
    );
    fs::write(&executable, source).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    (
        Runner {
            executable,
            directory: kph_process::directory(root).unwrap(),
            timeout: Duration::from_secs(3),
        },
        log,
    )
}

#[test]
fn process_pipeline_sends_both_requests_then_verifies_existence_and_cleans_up() {
    let root = tempfile::tempdir().unwrap();
    let (runner, log) = runner(root.path(), "", false);
    let workspace = runner.directory.path().to_owned();
    let plan = Plan::build(
        &plan_tests::profile(),
        Backend::Trustkernel,
        &plan_tests::properties(),
    )
    .unwrap();
    let Request::Trustkernel { items } = &plan.request else {
        panic!()
    };
    let mut session = backend::SessionInfo::default();
    trustkernel::execute(&runner, items, &mut session).unwrap();
    assert_eq!(session.ta_version.as_deref(), Some("3.0"));
    assert!(
        session
            .writes
            .iter()
            .all(|w| w.import_succeeded && w.exists == Some(true))
    );
    let calls: Vec<serde_json::Value> = fs::read_to_string(log)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        calls
            .iter()
            .map(|c| c["args"][1].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "ta_version",
            "import_data",
            "import_data",
            "verify_data",
            "verify_data"
        ]
    );
    for (call, item) in calls[1..3].iter().zip(items) {
        assert_eq!(call["hex"], hex::encode(&item.payload));
        assert_eq!(call["mode"], 0o600);
    }
    drop(runner);
    assert!(!workspace.exists());
}

#[test]
fn partial_writes_and_verification_failure_are_not_reported_as_success() {
    for (fail_tag, fail_verify, expected) in [
        ("attestation_ids", false, vec![true, false]),
        ("", true, vec![true, true]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (runner, _) = runner(root.path(), fail_tag, fail_verify);
        let plan = Plan::build(
            &plan_tests::profile(),
            Backend::Trustkernel,
            &plan_tests::properties(),
        )
        .unwrap();
        let Request::Trustkernel { items } = &plan.request else {
            panic!()
        };
        let mut session = backend::SessionInfo::default();
        let error = trustkernel::execute(&runner, items, &mut session).unwrap_err();
        assert_eq!(error::code_of(&error), "trustkernel_operation_failed");
        assert_eq!(
            session
                .writes
                .iter()
                .map(|w| w.import_succeeded)
                .collect::<Vec<_>>(),
            expected
        );
        assert_ne!(session.writes[0].exists, Some(true));
        if !fail_tag.is_empty() {
            assert!(
                session
                    .diagnostics
                    .last()
                    .unwrap()
                    .stderr
                    .contains("0xffff0005")
            );
            assert!(!error.to_string().contains("private diagnostic"));
        }
    }
}

#[test]
fn hung_and_missing_processes_release_their_workspace() {
    let root = tempfile::tempdir().unwrap();
    let executable = root.path().join("hang.sh");
    fs::write(&executable, "#!/bin/sh\nexec sleep 10\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    let mut runner = Runner {
        executable,
        directory: kph_process::directory(root.path()).unwrap(),
        timeout: Duration::from_millis(40),
    };
    let workspace = runner.directory.path().to_owned();
    let start = Instant::now();
    assert_eq!(
        error::code_of(&runner.run(&["-c", "ta_version"]).err().unwrap()),
        "provisioning_timeout"
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    runner.executable = root.path().join("missing");
    assert!(runner.run(&["-c", "ta_version"]).is_err());
    drop(runner);
    assert!(!workspace.exists());
}

#[test]
fn hash_pinning_snapshot_and_file_lock_protect_execution_inputs() {
    let root = tempfile::tempdir().unwrap();
    let ctx = duck_core::Context {
        paths: duck_core::AppPaths::for_root(root.path(), root.path()),
        sysroot: duck_core::Sysroot::new(root.path()),
    };
    let source = ctx.paths.root.join("kph_patched");
    let bytes = include_bytes!("../../../module/kph_patched");
    fs::write(&source, bytes).unwrap();
    let vendor = ctx.sysroot.path("/vendor/bin/kph");
    fs::create_dir_all(vendor.parent().unwrap()).unwrap();
    fs::write(&vendor, bytes).unwrap();
    fs::set_permissions(&vendor, fs::Permissions::from_mode(0o700)).unwrap();
    let executable = kph_artifact::select(&ctx).unwrap();
    let directory = kph_process::directory(&ctx.paths.tmp_dir).unwrap();
    let snapshot = kph_artifact::snapshot(&executable, directory.path()).unwrap();
    fs::write(&source, b"replaced").unwrap();
    assert_eq!(fs::read(snapshot).unwrap(), bytes);
    assert!(kph_artifact::select(&ctx).is_err());
    fs::remove_file(&source).unwrap();
    let target = root.path().join("elsewhere");
    fs::write(&target, bytes).unwrap();
    symlink(target, &source).unwrap();
    assert!(kph_artifact::select(&ctx).is_err());
    fs::remove_file(&source).unwrap();
    assert_eq!(kph_artifact::select(&ctx).unwrap().source, vendor);
    fs::set_permissions(&vendor, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(kph_artifact::select(&ctx).is_err());
    let lock = kph_process::lock(&ctx.paths.var_dir).unwrap();
    assert_eq!(
        error::code_of(&kph_process::lock(&ctx.paths.var_dir).err().unwrap()),
        "provisioning_busy"
    );
    drop(lock);
    assert!(kph_process::lock(&ctx.paths.var_dir).is_ok());
}
