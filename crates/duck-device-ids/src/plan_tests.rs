use super::*;
use crate::{
    backend, defaults,
    plan::{Plan, Request},
};
use duck_core::Sysroot;
use std::collections::BTreeMap;

pub(super) fn profile() -> DeviceIdsProfile {
    DeviceIdsProfile {
        brand: " Xelex ".into(),
        device: "Q25".into(),
        product: "Q25_EEA".into(),
        serial: "ABC123".into(),
        manufacturer: "Zinwa".into(),
        model: "Q25 Plus".into(),
        imei: "111111111111111".into(),
        imei2: "222222222222222".into(),
        ..DeviceIdsProfile::default()
    }
}

pub(super) fn properties() -> BTreeMap<String, String> {
    [
        ("ro.soc.manufacturer".into(), "MediaTek".into()),
        ("ro.vendor.mediatek.platform".into(), "MT6789".into()),
    ]
    .into()
}

#[test]
fn uses_shared_soc_evidence_and_preserves_vendor_defaults() {
    let root = tempfile::tempdir().unwrap();
    let root = Sysroot::new(root.path());
    let values = properties();
    let detected = defaults::detect(&values, &root);
    assert_eq!(detected.backend, Some(Backend::Trustkernel));
    assert_eq!(detected.profile.ta_name, "keymaster");
    assert_eq!(detected.profile.ta_path, "/vendor/app/t6");
    assert_eq!(
        backend::select(
            &[("ro.hardware.mediatek.platform".into(), "MT6789".into())].into(),
            &root
        )
        .unwrap(),
        Backend::Trustkernel
    );
    let qcom = [("ro.soc.manufacturer".into(), "QTI".into())].into();
    let detected = defaults::detect(&qcom, &root);
    assert_eq!(detected.backend, Some(Backend::Qseecom));
    assert_eq!(detected.profile.ta_name, "keymaster64");
    let mut conflict = values;
    conflict.insert("ro.hardware".into(), "qcom".into());
    assert!(backend::select(&conflict, &root).is_err());
    assert_eq!(defaults::detect(&conflict, &root).backend, None);
}

#[test]
fn trustkernel_requests_retain_both_targets_and_exact_null_terminated_bytes() {
    let mut profile = profile();
    profile.meid = "A0000000002321".into();
    let plan = Plan::build(&profile, Backend::Trustkernel, &properties()).unwrap();
    assert_eq!(plan.omitted_ids, ["MEID"]);
    assert!(plan.ids.iter().all(|id| id.label != "MEID"));
    let Request::Trustkernel { items } = &plan.request else {
        panic!()
    };
    assert_eq!(items.len(), 2);
    assert_eq!(
        items[0].payload,
        b"BRAND=Xelex MODEL=Q25 Plus PLATFORM=MT6789 PRODUCT=Q25_EEA\0"
    );
    assert_eq!(items[1].payload, b"BRAND=Xelex DEVICE=Q25 PRODUCT=Q25_EEA MANUFACTURER=Zinwa MODEL=Q25 Plus SERIAL=ABC123 IMEI=111111111111111 IMEI2=222222222222222\0");
    for item in items {
        assert_eq!((item.permission, item.flags), (42, 0));
        assert_eq!(
            item.uuid.split('-').map(str::len).collect::<Vec<_>>(),
            [8, 4, 4, 4, 12]
        );
    }
    let report = serde_json::to_value(&plan).unwrap();
    assert_eq!(
        hex::decode(report["request"]["items"][0]["payload"].as_str().unwrap()).unwrap(),
        items[0].payload
    );
}

#[test]
fn optional_serial_is_backend_specific_and_ambiguous_fields_are_rejected() {
    let mut profile = profile();
    profile.serial.clear();
    assert!(Plan::build(&profile, Backend::Qseecom, &properties()).is_err());
    assert!(Plan::build(&profile, Backend::Trustkernel, &properties()).is_ok());
    for value in ["x\0y", "x\ny", "x PRODUCT=other"] {
        profile.model = value.into();
        assert!(Plan::build(&profile, Backend::Trustkernel, &properties()).is_err());
    }
}

#[test]
fn private_reports_include_partial_state_and_the_exact_requests() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let paths = duck_core::AppPaths::for_root(root.path(), root.path());
    let profile = profile();
    let plan = Plan::build(&profile, Backend::Trustkernel, &properties()).unwrap();
    let report = report::Report::new(&paths).unwrap();
    let session = backend::SessionInfo {
        writes: vec![backend::WriteOutcome {
            tag: "rkp_flag".into(),
            import_succeeded: true,
            exists: None,
        }],
        ..Default::default()
    };
    report
        .write(
            &profile,
            &plan,
            &session,
            "failed",
            Some(&anyhow::anyhow!("second import failed")),
        )
        .unwrap();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(report.path()).unwrap()).unwrap();
    assert_eq!(saved["status"], "failed");
    assert_eq!(saved["request"]["items"].as_array().unwrap().len(), 2);
    assert_eq!(saved["writes"][0]["import_succeeded"], true);
    assert!(saved["writes"][0]["exists"].is_null());
    assert_eq!(
        std::fs::metadata(report.path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(report.path().parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
}
