//! TrustKernel device ID provisioning via the patched `kph` CLI.
//!
//! On MediaTek devices with TrustKernel TEE, attestation device IDs are stored
//! through the `kph` (KeyProvision Helper) tool rather than through Qualcomm's
//! QSEECom interface. Two separate secure-storage items are provisioned:
//!
//! - `rkp_flag`: consumed by the RKP subsystem at boot to populate
//!   `vendor.trustkernel.rkp.uploaded`.
//! - `attestation_ids`: consumed by the Keymaster TA when generating attestation
//!   certificates (OID 1.3.6.1.4.1.11129.2.1.17).
//!
//! The stock `kph` binary shipped on the Zinwa Q25 has a defective Gen 2 import
//! path: the 128-byte context buffer is not initialized, causing
//! `TEEC_ERROR_BAD_FORMAT` (0xffff0005). A patched binary fixes this with a
//! trampoline that zeroes the buffer and writes the required magic value (42).
//! See the mtk-tk-ref wiki for the full root-cause analysis.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::DeviceIdsProfile;

/// TA UUID for the rkp_flag data item (kmsetkey TA).
const UUID_RKP_FLAG: &str = "b46325e6-5c90-8252-2eada8e32e5180d6";

/// TA UUID for the attestation_ids data item.
const UUID_ATTESTATION_IDS: &str = "2ea702fa-17bc-4752-b3adb2871a772347";

/// Tag name for the rkp_flag item inside the TA's secure storage.
const TAG_RKP_FLAG: &str = "rkp_flag";

/// Tag name for the attestation_ids item.
const TAG_ATTESTATION_IDS: &str = "attestation_ids";

/// Search paths for the patched kph binary, checked in order. The module
/// directory is tried first (bundled with Duck ToolBox), then the conventional
/// staging location, and finally the stock vendor binary as a last resort (which
/// will fail on Gen 2 imports but still works for verify/read operations).
const KPH_SEARCH_PATHS: &[&str] = &[
    "/data/adb/modules/duck-toolbox/kph_patched",
    "/data/local/tmp/kph_patched",
    "/vendor/bin/kph",
];

/// Locate the kph binary on the device filesystem.
fn find_kph() -> Result<PathBuf> {
    for candidate in KPH_SEARCH_PATHS {
        let path = PathBuf::from(candidate);
        if path.exists() {
            return Ok(path);
        }
    }
    bail!(
        "patched kph binary not found; searched {}",
        KPH_SEARCH_PATHS.join(", ")
    );
}

/// Query the TrustKernel TA version via `kph -c ta_version`.
fn query_ta_version(kph: &PathBuf) -> Option<String> {
    let output = Command::new(kph)
        .args(["-c", "ta_version"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Output is typically "kph-ta:3.0" or similar.
    let version = stdout.trim().to_owned();
    if version.is_empty() { None } else { Some(version) }
}

/// Build the ASCII payload for the `rkp_flag` target.
///
/// Format: `BRAND=%s MODEL=%s PLATFORM=%s PRODUCT=%s`
fn build_rkp_flag_payload(profile: &DeviceIdsProfile) -> String {
    let platform = duck_platform::props::first(&[
        "ro.vendor.mediatek.platform",
        "ro.hardware.mediatek.platform",
        "ro.board.platform",
    ]);
    format!(
        "BRAND={} MODEL={} PLATFORM={} PRODUCT={}",
        profile.brand, profile.model, platform, profile.product
    )
}

/// Build the ASCII payload for the `attestation_ids` target.
///
/// Format: `BRAND=%s DEVICE=%s PRODUCT=%s MANUFACTURER=%s MODEL=%s [SERIAL=%s] [IMEI=%s] [IMEI2=%s]`
///
/// Optional fields are omitted when empty.
fn build_attestation_ids_payload(profile: &DeviceIdsProfile) -> String {
    let mut parts = vec![
        format!("BRAND={}", profile.brand),
        format!("DEVICE={}", profile.device),
        format!("PRODUCT={}", profile.product),
        format!("MANUFACTURER={}", profile.manufacturer),
        format!("MODEL={}", profile.model),
    ];

    for (key, value) in [
        ("SERIAL", &profile.serial),
        ("IMEI", &profile.imei),
        ("IMEI2", &profile.imei2),
    ] {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            parts.push(format!("{key}={trimmed}"));
        }
    }

    parts.join(" ")
}

/// Import a single data item into TrustKernel secure storage via kph.
///
/// The `-p` parameter is the payload size INCLUDING a null terminator, matching
/// `tee_check_keybox` behavior.
fn kph_import(kph: &PathBuf, uuid: &str, tag: &str, payload: &str) -> Result<String> {
    let tmp_dir = PathBuf::from("/data/local/tmp");
    let payload_file = tmp_dir.join(format!("dtb_{tag}.dat"));

    // Write payload to a temp file.
    let mut file = std::fs::File::create(&payload_file)
        .with_context(|| format!("create payload file {}", payload_file.display()))?;
    file.write_all(payload.as_bytes())
        .with_context(|| format!("write payload to {}", payload_file.display()))?;
    drop(file);

    let payload_size = payload.len() + 1; // +1 for null terminator per protocol

    let output = Command::new(kph)
        .args([
            "-c", "import_data",
            "-u", uuid,
            "-g", tag,
            "-i", &payload_file.display().to_string(),
            "-p", &payload_size.to_string(),
            "-f", "0",
        ])
        .output()
        .with_context(|| format!("execute kph import_data for {tag}"))?;

    // Clean up the temp file regardless of outcome.
    let _ = std::fs::remove_file(&payload_file);

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        bail!(
            "kph import_data for {tag} failed (exit {}): stdout=[{stdout}] stderr=[{stderr}]",
            output.status.code().unwrap_or(-1)
        );
    }

    Ok(stdout)
}

/// Verify that a data item exists in TrustKernel secure storage.
///
/// Uses the stock kph binary (or the patched one — verify_data does not need the
/// context buffer fix) via `kph -c verify_data`.
fn kph_verify(kph: &PathBuf, uuid: &str, tag: &str) -> Result<bool> {
    let output = Command::new(kph)
        .args(["-c", "verify_data", "-u", uuid, "-g", tag])
        .output()
        .with_context(|| format!("execute kph verify_data for {tag}"))?;

    // verify_data returns exit 0 when the item exists, non-zero otherwise.
    Ok(output.status.success())
}

/// Provision device IDs into TrustKernel TEE via the patched kph binary.
///
/// Both `rkp_flag` and `attestation_ids` targets are written. The required fields
/// from the profile must be non-empty; optional fields (serial, IMEI, IMEI2) are
/// included only when present.
pub(crate) fn provision(profile: &DeviceIdsProfile) -> Result<crate::qseecom::SessionInfo> {
    let kph = find_kph()?;

    let ta_version = query_ta_version(&kph);

    // Build payloads.
    let rkp_payload = build_rkp_flag_payload(profile);
    let ids_payload = build_attestation_ids_payload(profile);

    eprintln!("trustkernel: provisioning rkp_flag via {}", kph.display());
    eprintln!("trustkernel: rkp_flag payload: {rkp_payload}");
    let rkp_output = kph_import(&kph, UUID_RKP_FLAG, TAG_RKP_FLAG, &rkp_payload)
        .context("provision rkp_flag")?;

    eprintln!("trustkernel: provisioning attestation_ids via {}", kph.display());
    eprintln!("trustkernel: attestation_ids payload: {ids_payload}");
    let ids_output = kph_import(&kph, UUID_ATTESTATION_IDS, TAG_ATTESTATION_IDS, &ids_payload)
        .context("provision attestation_ids")?;

    // Verify both imports landed.
    let rkp_ok = kph_verify(&kph, UUID_RKP_FLAG, TAG_RKP_FLAG)
        .unwrap_or(false);
    let ids_ok = kph_verify(&kph, UUID_ATTESTATION_IDS, TAG_ATTESTATION_IDS)
        .unwrap_or(false);

    if !rkp_ok {
        bail!("post-import verification failed: rkp_flag not found in secure storage");
    }
    if !ids_ok {
        bail!("post-import verification failed: attestation_ids not found in secure storage");
    }

    eprintln!("trustkernel: both targets verified");

    // Combine output for the response field.
    let combined_output = format!("rkp_flag: {rkp_output}\nattestation_ids: {ids_output}");
    let response_hex = if combined_output.trim().is_empty() {
        None
    } else {
        Some(hex::encode(combined_output.trim().as_bytes()))
    };

    Ok(crate::qseecom::SessionInfo {
        loaded_library: Some(kph.display().to_string()),
        ta_api_version: None,
        ta_version,
        response_hex,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeviceIdsProfile;

    fn test_profile() -> DeviceIdsProfile {
        DeviceIdsProfile {
            brand: "Xelex".into(),
            device: "Q25".into(),
            product: "Q25_EEA".into(),
            serial: "ABC123".into(),
            manufacturer: "Zinwa".into(),
            model: "Q25".into(),
            imei: "111111111111111".into(),
            imei2: "222222222222222".into(),
            ..DeviceIdsProfile::default()
        }
    }

    #[test]
    fn attestation_ids_payload_includes_optional_fields() {
        let payload = build_attestation_ids_payload(&test_profile());
        assert!(payload.contains("BRAND=Xelex"));
        assert!(payload.contains("DEVICE=Q25"));
        assert!(payload.contains("PRODUCT=Q25_EEA"));
        assert!(payload.contains("MANUFACTURER=Zinwa"));
        assert!(payload.contains("MODEL=Q25"));
        assert!(payload.contains("SERIAL=ABC123"));
        assert!(payload.contains("IMEI=111111111111111"));
        assert!(payload.contains("IMEI2=222222222222222"));
    }

    #[test]
    fn attestation_ids_payload_omits_empty_optional_fields() {
        let mut profile = test_profile();
        profile.serial = String::new();
        profile.imei = String::new();
        profile.imei2 = String::new();
        let payload = build_attestation_ids_payload(&profile);
        assert!(!payload.contains("SERIAL="));
        assert!(!payload.contains("IMEI="));
        assert!(!payload.contains("IMEI2="));
        // Required fields remain.
        assert!(payload.contains("BRAND=Xelex"));
        assert!(payload.contains("MANUFACTURER=Zinwa"));
    }
}
