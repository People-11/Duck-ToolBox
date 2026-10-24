use crate::{
    Backend,
    plan::Plan,
    trustkernel_native,
    trustkernel_worker::{Call, Reply},
};
use std::{fs, path::PathBuf, process::Command};

fn library(source: &str) -> (tempfile::TempDir, PathBuf) {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("fixture.c");
    let output = directory.path().join("libfixture.so");
    fs::write(&input, source).unwrap();
    let result = Command::new("cc")
        .args(["-shared", "-fPIC", "-Wall", "-Werror", "-o"])
        .arg(&output)
        .arg(input)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    (directory, output)
}

fn items() -> Vec<crate::plan::DataItem> {
    let plan = Plan::build(
        &crate::plan_tests::profile(),
        Backend::Trustkernel,
        &crate::plan_tests::properties(),
    )
    .unwrap();
    let crate::plan::Request::Trustkernel { items } = plan.request else {
        panic!()
    };
    items
}

#[test]
fn uuid_abi_passes_context_payload_lengths_and_preserves_vendor_status() {
    let (_directory, library) = library(
        r#"
#include <stdint.h>
#include <string.h>
int kph_ta_version(char *out, uint32_t len) {
    if (len != 32) return -1;
    memcpy(out, "3.0", 4); return 0;
}
int kph_import_ta_data_by_uuid(const char *uuid, const char *tag,
    const unsigned char *context, uint32_t context_len,
    const unsigned char *payload, uint32_t payload_len, uint32_t storage) {
    if (context_len != 128 || context[0] != 42 || storage != 0) return 10;
    for (int i = 1; i < 128; i++) if (context[i] != 0) return 11;
    if (payload_len != strlen((const char *)payload) + 1 || payload[payload_len - 1] != 0) return 12;
    if (strncmp((const char *)payload, "BRAND=", 6)) return 13;
    if (!strcmp(tag, "rkp_flag") && !strcmp(uuid, "b46325e6-5c90-8252-2ead-a8e32e5180d6")) return 0;
    if (!strcmp(tag, "attestation_ids") && !strcmp(uuid, "2ea702fa-17bc-4752-b3ad-b2871a772347")) return (int)0xffff0005u;
    return 14;
}
int kph_verify_data_by_uuid(const char *uuid, const char *tag) {
    return strcmp(uuid, "b46325e6-5c90-8252-2ead-a8e32e5180d6") || strcmp(tag, "rkp_flag") ? -9 : 0;
}
"#,
    );
    assert!(
        matches!(trustkernel_native::call(&library, &Call::Version).unwrap(),
        Reply::Returned { status: 0, version: Some(v), .. } if v == "3.0")
    );
    for (item, expected) in items().into_iter().zip([0, -65531]) {
        assert!(
            matches!(trustkernel_native::call(&library, &Call::Import { item }).unwrap(),
            Reply::Returned { status, .. } if status == expected)
        );
    }
    let item = items().remove(0);
    assert!(matches!(
        trustkernel_native::call(
            &library,
            &Call::Verify {
                uuid: item.uuid,
                tag: item.tag
            }
        )
        .unwrap(),
        Reply::Returned { status: 0, .. }
    ));
}

#[test]
fn incompatible_legacy_exports_fail_before_import() {
    let (_directory, library) = library(
        r#"
int kph_import_ta_data(void) { return 0; }
int kph_verify_ta_data2(void) { return 0; }
"#,
    );
    let error = trustkernel_native::call(
        &library,
        &Call::Import {
            item: items().remove(0),
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("UUID-addressed KPH import"));
}

#[test]
fn verification_export_is_required_before_any_write() {
    let (_directory, library) = library(
        r#"
#include <stdlib.h>
int kph_import_ta_data_by_uuid(void) { abort(); }
"#,
    );
    let error = trustkernel_native::call(
        &library,
        &Call::Import {
            item: items().remove(0),
        },
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("UUID-addressed KPH verification"));
}

#[test]
fn invalid_target_or_context_is_rejected_before_loading_vendor_code() {
    let mut item = items().remove(0);
    for change in 0..4 {
        match change {
            0 => item.permission = 0,
            1 => {
                item.permission = 42;
                item.flags = 1;
            }
            2 => {
                item.flags = 0;
                item.payload.pop();
            }
            _ => {
                item.payload.push(0);
                item.uuid = "unsupported".into();
            }
        }
        assert!(
            trustkernel_native::call(
                std::path::Path::new("missing"),
                &Call::Import { item: item.clone() }
            )
            .is_err()
        );
    }
}
