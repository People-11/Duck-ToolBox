// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: read-only system/config comparison, without inferring a server verdict.
use crate::{Engine, command, config, rules};
use anyhow::Result;
use serde_json::{Value, json};
use std::{collections::BTreeMap, process::Command, time::Duration};

fn properties(raw: &str) -> BTreeMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let (key, value) = line
                .strip_prefix('[')?
                .strip_suffix(']')?
                .split_once("]: [")?;
            Some((key.into(), value.into()))
        })
        .collect()
}
pub fn inspect(engine: &Engine) -> Result<Value> {
    let current = config::current(engine)?;
    let mut error = None;
    let system = if engine.sysroot == std::path::Path::new("/") {
        match command::run(&mut Command::new("getprop"), Duration::from_secs(2)) {
            Ok(output) if output.status.success() => {
                properties(&String::from_utf8_lossy(&output.stdout))
            }
            _ => {
                error = Some("system_properties_unavailable");
                BTreeMap::new()
            }
        }
    } else {
        BTreeMap::new()
    };
    let rows: Vec<Value> = rules::BUILD_PROPERTIES.iter().map(|(field, property)| {
        json!({"field":field,"property":property,"system":system.get(*property),"saved_build":current["entries"].get(*field),"saved_property":current["entries"].get(*property),"observed":null})
    }).collect();
    Ok(
        json!({"rows":rows,"revision":current["revision"],"read_error":error,"runtime_observed":false,"verdict_verified":false}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn separates_saved_values_from_observation_and_never_writes() {
        let root = tempfile::tempdir().unwrap();
        let engine = Engine {
            data: root.path().join("data"),
            module: root.path().join("module"),
            sysroot: root.path().join("sys"),
        };
        let result = inspect(&engine).unwrap();
        assert!(!engine.data.exists());
        assert_eq!(result["runtime_observed"], false);
        assert!(
            result["rows"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["system"].is_null() && r["observed"].is_null())
        );
        assert_eq!(
            properties("[ro.product.model]: [A]: [B]\ninvalid")["ro.product.model"],
            "A]: [B"
        );
    }
}
