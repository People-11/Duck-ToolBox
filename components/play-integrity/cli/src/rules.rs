// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: non-mutating configuration consistency diagnostics.
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub const BUILD_PROPERTIES: &[(&str, &str)] = &[
    ("BRAND", "ro.product.brand"),
    ("MANUFACTURER", "ro.product.manufacturer"),
    ("MODEL", "ro.product.model"),
    ("DEVICE", "ro.product.device"),
    ("PRODUCT", "ro.product.name"),
    ("FINGERPRINT", "ro.build.fingerprint"),
    ("ID", "ro.build.id"),
    ("RELEASE", "ro.build.version.release"),
    ("INCREMENTAL", "ro.build.version.incremental"),
    ("TYPE", "ro.build.type"),
    ("TAGS", "ro.build.tags"),
    ("SECURITY_PATCH", "ro.build.version.security_patch"),
    ("DEVICE_INITIAL_SDK_INT", "ro.product.first_api_level"),
];

pub fn inspect(entries: &BTreeMap<String, String>) -> Vec<Value> {
    let mut warnings = Vec::new();
    for key in ["DEVICE_INITIAL_SDK_INT", "ro.board.first_api_level"] {
        if entries.get(key).is_none_or(String::is_empty) {
            warnings.push(json!({"code":"missing_api","fields":[key]}));
        }
    }
    // Build.java reads these properties into separate static fields. A process-local
    // field override does not also replace a later SystemProperties read.
    for &(field, property) in BUILD_PROPERTIES {
        if let (Some(a), Some(b)) = (entries.get(field), entries.get(property))
            && !a.is_empty()
            && !b.is_empty()
            && a != b
        {
            warnings.push(json!({"code":"mismatch","fields":[field,property]}));
        }
    }
    if entries
        .get("spoofProvider")
        .is_none_or(|value| value.is_empty() || value == "1")
    {
        warnings.push(json!({"code":"provider","fields":["spoofProvider"]}));
    }
    for key in entries.keys().filter(|key| key.contains('*')) {
        if !key.starts_with('*') || key[1..].contains('*') {
            warnings.push(json!({"code":"wildcard","fields":[key]}));
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_do_not_rewrite_expert_configuration() {
        let entries = crate::config::parse(
            "MODEL=A\nro.product.model=B\nro.*.name=x\nspoofProvider=1",
            "prop",
        )
        .unwrap();
        let before = entries.clone();
        let warnings = inspect(&entries);
        for code in ["missing_api", "mismatch", "provider", "wildcard"] {
            assert!(warnings.iter().any(|item| item["code"] == code));
        }
        assert_eq!(entries, before);
    }
}
