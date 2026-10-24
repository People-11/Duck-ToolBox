// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: restrict ROM hiding to individual application/overlay entries.
use anyhow::{Result, bail};
pub fn validate_list(list: &str) -> Result<()> {
    for path in list
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
    {
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() != 4
            || !parts[0].is_empty()
            || !["system", "product", "system_ext", "vendor", "odm", "oem"].contains(&parts[1])
            || !["app", "priv-app", "overlay"].contains(&parts[2])
            || parts[3].is_empty()
            || parts[3] == "."
            || parts[3] == ".."
            || (parts[2] == "overlay" && (parts[3] == "config" || parts[3].contains('*')))
            || !parts[3]
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.+-*".contains(&c))
        {
            bail!("invalid ROM replacement path (expected a single app/overlay entry): {path}");
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_roots_and_parent_expansions() {
        for path in [
            "/",
            "/system/",
            "/system/app",
            "/system/app/",
            "/system/*",
            "/system/app/..",
            "/system/app/../*",
            "/system/app/A/..",
            "/system/overlay/config",
            "/system/overlay/*",
            "/data/app/A",
            "/product/app/A;id",
        ] {
            assert!(validate_list(path).is_err(), "{path}");
        }
        for path in [
            "/product/app/XiaomiEUInject",
            "/system/priv-app/A",
            "/system_ext/app/helluvaProductDevice*",
            "/system_ext/overlay/CertifiedPropsOverlay.apk",
        ] {
            validate_list(path).unwrap();
        }
        validate_list(include_str!("../../scripts/app_replace_list.txt")).unwrap();
    }
}
