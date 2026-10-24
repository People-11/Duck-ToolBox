use super::*;
use crate::{
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
path = pathlib.Path(sys.argv[sys.argv.index('--exchange')+1])
request = json.loads((path/'request.json').read_text())
call = request['call']
entry = {{'operation': call['operation'], 'mode': (path/'request.json').stat().st_mode & 0o777}}
status = 0
version = None
if call['operation'] == 'version': version = '3.0'
elif call['operation'] == 'import':
    item = call['item']
    entry['hex'] = item['payload']
    assert item['permission'] == 42 and item['flags'] == 0
    assert str(uuid.UUID(item['uuid'])) == item['uuid']
    if item['tag'] == {fail_tag}:
        print('TEEC 0xffff0005: private diagnostic', file=sys.stderr)
        status = -65531
elif call['operation'] == 'verify' and {fail_verify}: status = 8
with open({log}, 'a') as log: log.write(json.dumps(entry)+'\n')
(path/'reply.json').write_text(json.dumps({{'result': 'returned', 'library': request['library'], 'status': status, 'version': version}}))
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
    trustkernel::execute(
        &runner,
        std::path::Path::new("/vendor/lib64/libkphproxy.so"),
        items,
        &mut session,
    )
    .unwrap();
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
            .map(|c| c["operation"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["version", "import", "import", "verify", "verify"]
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
        let error = trustkernel::execute(
            &runner,
            std::path::Path::new("/vendor/lib64/libkphproxy.so"),
            items,
            &mut session,
        )
        .unwrap_err();
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
fn file_lock_serializes_operations_and_rejects_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let lock = kph_process::lock(root.path()).unwrap();
    assert_eq!(
        error::code_of(&kph_process::lock(root.path()).err().unwrap()),
        "provisioning_busy"
    );
    drop(lock);
    assert!(kph_process::lock(root.path()).is_ok());
    fs::remove_file(root.path().join("device-ids.lock")).unwrap();
    symlink(
        root.path().join("elsewhere"),
        root.path().join("device-ids.lock"),
    )
    .unwrap();
    assert!(kph_process::lock(root.path()).is_err());
}

#[test]
fn exchange_files_are_bounded_and_cannot_be_symlinks() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    fs::write(&source, b"12345").unwrap();
    assert!(trustkernel_worker::read(&source, 4).is_err());
    assert_eq!(trustkernel_worker::read(&source, 5).unwrap(), b"12345");
    let link = root.path().join("link");
    symlink(source, &link).unwrap();
    assert!(trustkernel_worker::read(&link, 5).is_err());
    assert!(trustkernel_worker::read(root.path(), 5).is_err());
}

#[test]
fn missing_reply_does_not_reuse_a_previous_success() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, _) = runner(root.path(), "", false);
    let request = trustkernel_worker::Request {
        library: "/vendor/lib64/libkphproxy.so".into(),
        call: trustkernel_worker::Call::Version,
    };
    let mut session = backend::SessionInfo::default();
    trustkernel_worker::invoke(&runner, &request, &mut session, "version", None).unwrap();
    let executable = root.path().join("empty.sh");
    fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    runner.executable = executable;
    let error =
        trustkernel_worker::invoke(&runner, &request, &mut session, "version", None).unwrap_err();
    assert_eq!(error::code_of(&error), "trustkernel_transport_failed");
}

#[test]
fn crashed_or_invalid_workers_preserve_attempted_write_state() {
    for behavior in ["crash", "malformed", "wrong_library", "oversize"] {
        let root = tempfile::tempdir().unwrap();
        let (runner, _) = runner(root.path(), "", false);
        let source = fs::read_to_string(&runner.executable).unwrap();
        let fault = match behavior {
            "crash" => "sys.exit(139)",
            "malformed" => "(path/'reply.json').write_text('invalid'); sys.exit(0)",
            "oversize" => "(path/'reply.json').write_text('x'*17000); sys.exit(0)",
            _ => "request['library'] = '/different/libkphproxy.so'",
        };
        fs::write(&runner.executable, source.replace("call = request['call']",
            &format!("call = request['call']\nif call['operation'] == 'import' and call['item']['tag'] == 'attestation_ids':\n    {fault}"))).unwrap();
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
        let error = trustkernel::execute(
            &runner,
            std::path::Path::new("/vendor/lib64/libkphproxy.so"),
            items,
            &mut session,
        )
        .unwrap_err();
        assert_eq!(
            error::code_of(&error),
            "trustkernel_transport_failed",
            "{behavior}"
        );
        assert_eq!(session.writes.len(), 2);
        assert!(session.writes[0].import_succeeded);
        assert!(!session.writes[1].import_succeeded);
        assert!(session.writes.iter().all(|write| write.exists.is_none()));
    }
}

#[test]
fn timed_out_worker_retains_private_diagnostics_and_raw_vendor_status() {
    let root = tempfile::tempdir().unwrap();
    let (mut runner, _) = runner(root.path(), "attestation_ids", false);
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
    trustkernel::execute(
        &runner,
        std::path::Path::new("/vendor/lib64/libkphproxy.so"),
        items,
        &mut session,
    )
    .unwrap_err();
    assert!(
        session
            .diagnostics
            .last()
            .unwrap()
            .status
            .contains("0xffff0005")
    );

    let source = fs::read_to_string(&runner.executable).unwrap().replace(
        "call = request['call']",
        "call = request['call']\nif call['operation'] == 'import' and call['item']['tag'] == 'attestation_ids':\n    import time\n    print('private diagnostic before timeout', file=sys.stderr, flush=True)\n    time.sleep(10)",
    );
    fs::write(&runner.executable, source).unwrap();
    runner.timeout = Duration::from_secs(1);
    let mut session = backend::SessionInfo::default();
    let error = trustkernel::execute(
        &runner,
        std::path::Path::new("/vendor/lib64/libkphproxy.so"),
        items,
        &mut session,
    )
    .unwrap_err();
    assert_eq!(error::code_of(&error), "provisioning_timeout");
    assert!(!error.to_string().contains("private diagnostic"));
    assert!(
        session
            .diagnostics
            .last()
            .unwrap()
            .stderr
            .contains("private diagnostic before timeout")
    );
    assert_eq!(
        session.diagnostics.last().unwrap().tag.as_deref(),
        Some("attestation_ids")
    );
    assert_eq!(session.writes.len(), 2);
    assert!(session.writes[0].import_succeeded);
    assert!(!session.writes[1].import_succeeded);
}

#[test]
fn oversized_vendor_output_fails_with_bounded_private_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let (runner, _) = runner(root.path(), "", false);
    let source = fs::read_to_string(&runner.executable).unwrap().replace(
        "call = request['call']",
        "call = request['call']\nprint('private vendor diagnostic:' + 'x'*9000, file=sys.stderr, flush=True)",
    );
    fs::write(&runner.executable, source).unwrap();
    let request = trustkernel_worker::Request {
        library: "/vendor/lib64/libkphproxy.so".into(),
        call: trustkernel_worker::Call::Version,
    };
    let mut session = backend::SessionInfo::default();
    let error =
        trustkernel_worker::invoke(&runner, &request, &mut session, "version", None).unwrap_err();
    assert!(!error.to_string().contains("private vendor diagnostic"));
    let diagnostic = session.diagnostics.last().unwrap();
    assert!(diagnostic.stderr.starts_with("private vendor diagnostic:"));
    assert_eq!(diagnostic.stderr.len(), 8192);
    assert!(diagnostic.status.contains("exceeds 8192 bytes"));
}
