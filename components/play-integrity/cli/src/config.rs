// SPDX-License-Identifier: GPL-3.0-only
// Derived from Play Integrity Fork migrate.sh and native configuration handling.
// Upstream 23ec56c2440c4fd1a5e893a23ef7861d6366a828; modified 2026-10-24:
// bounded parsing, non-destructive migration, preserved custom fields and previews.
// Modified 2026-10-26: migration always fills missing fields; spoofing switches are fixed in the
// injected code, so configurations carry no switch keys or defaults.
use crate::{Engine, files};
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub type Entries = BTreeMap<String, String>;

pub fn parse(raw: &str, format: &str) -> Result<Entries> {
    if !["auto", "prop", "json"].contains(&format) {
        bail!("unsupported format: {format}");
    }
    if raw.len() > 64 * 1024 {
        bail!("configuration exceeds 64 KiB");
    }
    let mut entries = Entries::new();
    if format == "json" || raw.trim_start().starts_with('{') {
        // Legacy PIF JSON permits // and /* */ comments, but not inside string values.
        let clean = strip_comments(raw)?;
        let value: Value = serde_json::from_str(&clean)?;
        let object = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("expected JSON object"))?;
        for (key, value) in object {
            let text = match value {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => (*b as u8).to_string(),
                _ => bail!("{key}: expected scalar"),
            };
            entries.insert(key.clone(), text);
        }
    } else if format == "prop" || format == "auto" {
        for (number, line) in raw.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| anyhow::anyhow!("line {}: expected key=value", number + 1))?;
            entries.insert(
                key.trim().into(),
                value.split('#').next().unwrap_or_default().trim().into(),
            );
        }
    } else {
        bail!("unsupported format: {format}");
    }
    validate(&entries)?;
    Ok(entries)
}
fn strip_comments(raw: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = raw.chars().peekable();
    let mut string = false;
    let mut escape = false;
    while let Some(c) = chars.next() {
        if string {
            out.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                string = false;
            }
            continue;
        }
        if c == '"' {
            string = true;
            out.push(c);
        } else if c == '/' && chars.peek() == Some(&'/') {
            chars.next();
            for ch in chars.by_ref() {
                if ch == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            let mut closed = false;
            while let Some(ch) = chars.next() {
                if ch == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    closed = true;
                    break;
                }
                if ch == '\n' {
                    out.push('\n');
                }
            }
            if !closed {
                bail!("unterminated JSON comment");
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    Ok(out)
}
pub fn validate(entries: &Entries) -> Result<()> {
    if entries.len() > 512 {
        bail!("too many entries (maximum 512)");
    }
    for (key, value) in entries {
        if key.is_empty()
            || key.len() > 128
            || !key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._*".contains(&b))
        {
            bail!("invalid field name: {key}");
        }
        if value.len() > 4096 || value.contains(['\0', '\n', '\r']) {
            bail!("invalid value: {key}");
        }
        if matches!(
            key.as_str(),
            "DEVICE_INITIAL_SDK_INT"
                | "SDK_INT"
                | "PREVIEW_SDK_INT"
                | "RESOURCES_SDK_INT"
                | "MEDIA_PERFORMANCE_CLASS"
                | "ro.product.first_api_level"
                | "ro.board.first_api_level"
        ) && !value.is_empty()
            && value.parse::<i32>().ok().is_none_or(|number| number < 0)
        {
            bail!("{key}: expected a non-negative 32-bit integer or an empty value");
        }
    }
    Ok(())
}
/// Fills Build fields from the fingerprint and properties from Build fields, without
/// replacing present values.
pub fn migrate(entries: &mut Entries) {
    let fingerprint = entries.get("FINGERPRINT").cloned().unwrap_or_default();
    let pieces: Vec<_> = fingerprint.split(['/', ':']).collect();
    if pieces.len() == 8 {
        for (key, value) in [
            "BRAND",
            "PRODUCT",
            "DEVICE",
            "RELEASE",
            "ID",
            "INCREMENTAL",
            "TYPE",
            "TAGS",
        ]
        .into_iter()
        .zip(pieces)
        {
            if entries.get(key).is_none_or(String::is_empty) {
                entries.insert(key.into(), value.into());
            }
        }
    }
    for &(field, prop) in crate::rules::BUILD_PROPERTIES {
        if let Some(value) = entries.get(field).cloned() {
            entries.entry(prop.into()).or_insert(value);
        }
    }
}
pub fn serialize(entries: &Entries, format: &str) -> Result<String> {
    if format == "json" {
        Ok(serde_json::to_string_pretty(entries)? + "\n")
    } else {
        Ok(entries.iter().map(|(k, v)| format!("{k}={v}\n")).collect())
    }
}
fn display(entries: &Entries) -> Result<String> {
    serialize(
        entries,
        if entries.values().any(|v| v.contains('#')) {
            "json"
        } else {
            "prop"
        },
    )
}
pub fn current(engine: &Engine) -> Result<Value> {
    let content = files::read(&engine.data.join("custom.pif.json"))?;
    let entries = parse(&content, if content.is_empty() { "prop" } else { "json" })?;
    Ok(
        json!({"content":display(&entries)?,"json":serialize(&entries,"json")?,"entries":entries,"revision":files::hash(content.as_bytes())}),
    )
}
pub fn preview(engine: &Engine, input: &Value) -> Result<Value> {
    let raw = crate::text(input, "content")?;
    let format = input["format"].as_str().unwrap_or("auto");
    let entries = parse(raw, format)?;
    let old = current(engine)?;
    let before: Entries = serde_json::from_value(old["entries"].clone())?;
    if serialize(&entries, "json")?.len() > 64 * 1024 {
        bail!("serialized configuration exceeds 64 KiB");
    }
    let mut keys: Vec<_> = before.keys().chain(entries.keys()).collect();
    keys.sort();
    keys.dedup();
    let changes: Vec<_> = keys
        .into_iter()
        .filter(|key| before.get(*key) != entries.get(*key))
        .map(|key| json!({"key":key,"before":before.get(key),"after":entries.get(key)}))
        .collect();
    Ok(
        json!({"content":display(&entries)?,"json":serialize(&entries,"json")?,"entries":entries,"changes":changes,"revision":old["revision"],"warnings":crate::rules::inspect(&entries),"suggested_patch":crate::rules::suggested_patch(&entries)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_and_unknown_fields_survive_migration() {
        let mut e=parse("{\n// old\n\"FINGERPRINT\":\"google/p/d:16/ID/123:user/release-keys\",\"EXTRA\":\"https://a/b\"}","json").unwrap();
        migrate(&mut e);
        assert_eq!(e["RELEASE"], "16");
        assert_eq!(e["EXTRA"], "https://a/b");
        assert_eq!(e["ro.build.fingerprint"], e["FINGERPRINT"]);
    }
    #[test]
    fn prop_last_line_wildcards_and_invalid_values() {
        let e = parse(
            "MODEL=a\r\nMODEL=b # comment\n*.security_patch=2026-10-05",
            "prop",
        )
        .unwrap();
        assert_eq!(e["MODEL"], "b");
        assert!(e.contains_key("*.security_patch"));
        assert!(parse("bad-key=x", "prop").is_err());
        assert!(parse("{\"x\":[]}", "json").is_err());
        assert!(parse("DEVICE_INITIAL_SDK_INT=invalid", "prop").is_err());
        assert!(parse("ro.board.first_api_level=2147483648", "prop").is_err());
        assert!(parse("ro.board.first_api_level=202404", "prop").is_ok());
        assert!(parse("{}", "yaml").is_err());
    }
    #[test]
    fn migration_keeps_api_choices_and_never_invents_launch_levels() {
        let mut entries = parse("MODEL=A", "prop").unwrap();
        migrate(&mut entries);
        assert!(!entries.contains_key("DEVICE_INITIAL_SDK_INT"));
        assert!(!entries.contains_key("ro.board.first_api_level"));
        entries.insert("DEVICE_INITIAL_SDK_INT".into(), "36".into());
        entries.insert("ro.board.first_api_level".into(), "35".into());
        migrate(&mut entries);
        assert_eq!(entries["DEVICE_INITIAL_SDK_INT"], "36");
        assert_eq!(entries["ro.product.first_api_level"], "36");
        assert_eq!(entries["ro.board.first_api_level"], "35");
    }
}
