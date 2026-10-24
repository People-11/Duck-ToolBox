//! TrustKernel device ID provisioning via direct FFI to `libkphproxy.so`.
//!
//! On MediaTek devices with TrustKernel TEE, attestation device IDs are stored
//! through the KPH (KeyProvision Helper) library rather than through Qualcomm's
//! QSEECom interface. Two separate secure-storage items are provisioned:
//!
//! - `rkp_flag`: consumed by the RKP subsystem at boot to populate
//!   `vendor.trustkernel.rkp.uploaded`.
//! - `attestation_ids`: consumed by the Keymaster TA when generating attestation
//!   certificates (OID 1.3.6.1.4.1.11129.2.1.17).
//!
//! The stock `kph` binary shipped on the Zinwa Q25 has a defective Gen 2 import
//! path: the 128-byte context buffer passed to `kph_import_ta_data` is not
//! initialized, causing `TEEC_ERROR_BAD_FORMAT` (0xffff0005). This module
//! applies the fix inline: the buffer is zeroed and its first four bytes are set
//! to the magic value 42 (0x2A as a little-endian u32).
//!
//! No vendor binaries are shipped. `libkphproxy.so` is loaded at runtime from
//! `/vendor/lib64/` via `dlopen`, keeping the build MIT-compatible. The library
//! is present on every TrustKernel device as part of the vendor partition.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_uchar, c_uint};

use anyhow::{Context, Result, bail};
use libloading::Library;

use crate::DeviceIdsProfile;

/// TA UUID for the rkp_flag data item (kmsetkey TA).
const UUID_RKP_FLAG: &str = "b46325e6-5c90-8252-2eada8e32e5180d6";

/// TA UUID for the attestation_ids data item.
const UUID_ATTESTATION_IDS: &str = "2ea702fa-17bc-4752-b3adb2871a772347";

/// Tag name for the rkp_flag item inside the TA's secure storage.
const TAG_RKP_FLAG: &str = "rkp_flag";

/// Tag name for the attestation_ids item.
const TAG_ATTESTATION_IDS: &str = "attestation_ids";

/// Size of the context buffer expected by `kph_import_ta_data`.
const KPH_CONTEXT_BUF_SIZE: usize = 128;

/// Magic value written to the first four bytes of the context buffer. Without
/// this initialization the Gen 2 import path returns TEEC_ERROR_BAD_FORMAT.
const KPH_CONTEXT_MAGIC: u32 = 42;

/// Search paths for `libkphproxy.so`, checked in order.
const LIB_SEARCH_PATHS: &[&str] = &[
    "/vendor/lib64/libkphproxy.so",
    "/vendor/lib/libkphproxy.so",
];

// FFI function signatures matching the exports of libkphproxy.so.
//
// int kph_import_ta_data(
//     const char *uuid, const char *tag,
//     const unsigned char *data, unsigned int data_len,
//     unsigned char *context, unsigned int storage_type
// );
type KphImportTaData = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const c_uchar,
    c_uint,
    *mut c_uchar,
    c_uint,
) -> c_int;

// int kph_verify_ta_data2(const char *uuid, const char *tag);
type KphVerifyTaData2 = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;

// int kph_remove_ta_data2(const char *uuid, const char *tag);
type KphRemoveTaData2 = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;

/// Loaded KPH library handle and resolved function pointers.
struct KphApi {
    /// Retained to keep the library mapped for the lifetime of the API handle.
    _lib: Library,
    import_ta_data: KphImportTaData,
    verify_ta_data2: KphVerifyTaData2,
    #[allow(dead_code)]
    remove_ta_data2: KphRemoveTaData2,
    lib_path: String,
}

impl KphApi {
    /// Load `libkphproxy.so` from the vendor partition and resolve symbols.
    fn load() -> Result<Self> {
        let mut last_err = None;
        for candidate in LIB_SEARCH_PATHS {
            let path = std::path::Path::new(candidate);
            if !path.exists() {
                continue;
            }
            match unsafe { Library::new(candidate) } {
                Ok(lib) => {
                    let api = unsafe {
                        let import: libloading::Symbol<'_, KphImportTaData> =
                            lib.get(b"kph_import_ta_data\0")
                                .context("resolve kph_import_ta_data")?;
                        let verify: libloading::Symbol<'_, KphVerifyTaData2> =
                            lib.get(b"kph_verify_ta_data2\0")
                                .context("resolve kph_verify_ta_data2")?;
                        let remove: libloading::Symbol<'_, KphRemoveTaData2> =
                            lib.get(b"kph_remove_ta_data2\0")
                                .context("resolve kph_remove_ta_data2")?;
                        KphApi {
                            import_ta_data: *import,
                            verify_ta_data2: *verify,
                            remove_ta_data2: *remove,
                            lib_path: candidate.to_string(),
                            _lib: lib,
                        }
                    };
                    return Ok(api);
                }
                Err(e) => {
                    last_err = Some(e);
                }
            }
        }
        match last_err {
            Some(e) => bail!("failed to load libkphproxy.so: {e}"),
            None => bail!(
                "libkphproxy.so not found; searched {}",
                LIB_SEARCH_PATHS.join(", ")
            ),
        }
    }
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
/// Optional fields are omitted when empty. The field order matches the
/// TrustKernel TA's positional parser — MANUFACTURER and MODEL must follow
/// PRODUCT immediately, before any optional fields.
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

/// Import a data item into TrustKernel secure storage via `kph_import_ta_data`.
///
/// The context buffer is initialized with the magic value to work around the
/// Gen 2 bug in the stock kph binary. The data buffer includes a null terminator
/// appended after the payload bytes, matching `tee_check_keybox` expectations.
fn kph_import(api: &KphApi, uuid: &str, tag: &str, payload: &str) -> Result<()> {
    let uuid_c = CString::new(uuid).context("null byte in UUID")?;
    let tag_c = CString::new(tag).context("null byte in tag")?;

    // Data buffer: payload bytes followed by a null terminator.
    let mut data_buf: Vec<u8> = payload.as_bytes().to_vec();
    data_buf.push(0);
    let data_len = data_buf.len() as c_uint;

    // Context buffer: 128 bytes, first 4 set to magic value 42 (LE), rest zeroed.
    let mut context = [0u8; KPH_CONTEXT_BUF_SIZE];
    context[..4].copy_from_slice(&KPH_CONTEXT_MAGIC.to_le_bytes());

    let storage_type: c_uint = 0;

    let rc = unsafe {
        (api.import_ta_data)(
            uuid_c.as_ptr(),
            tag_c.as_ptr(),
            data_buf.as_ptr(),
            data_len,
            context.as_mut_ptr(),
            storage_type,
        )
    };

    if rc != 0 {
        bail!(
            "kph_import_ta_data for {tag} returned {rc} (0x{rc:08x})",
            rc = rc
        );
    }
    Ok(())
}

/// Verify that a data item exists in TrustKernel secure storage.
fn kph_verify(api: &KphApi, uuid: &str, tag: &str) -> Result<bool> {
    let uuid_c = CString::new(uuid).context("null byte in UUID")?;
    let tag_c = CString::new(tag).context("null byte in tag")?;

    let rc = unsafe { (api.verify_ta_data2)(uuid_c.as_ptr(), tag_c.as_ptr()) };
    Ok(rc == 0)
}

/// Provision device IDs into TrustKernel TEE via direct FFI to libkphproxy.so.
///
/// Both `rkp_flag` and `attestation_ids` targets are written. The 128-byte
/// context buffer fix is applied inline, eliminating the need for a patched
/// `kph` binary on the device.
pub(crate) fn provision(profile: &DeviceIdsProfile) -> Result<crate::SessionInfo> {
    let api = KphApi::load()?;

    let rkp_payload = build_rkp_flag_payload(profile);
    let ids_payload = build_attestation_ids_payload(profile);

    eprintln!("trustkernel: provisioning rkp_flag via {}", api.lib_path);
    eprintln!("trustkernel: rkp_flag payload: {rkp_payload}");
    kph_import(&api, UUID_RKP_FLAG, TAG_RKP_FLAG, &rkp_payload)
        .context("provision rkp_flag")?;

    eprintln!("trustkernel: provisioning attestation_ids via {}", api.lib_path);
    eprintln!("trustkernel: attestation_ids payload: {ids_payload}");
    kph_import(&api, UUID_ATTESTATION_IDS, TAG_ATTESTATION_IDS, &ids_payload)
        .context("provision attestation_ids")?;

    let rkp_ok = kph_verify(&api, UUID_RKP_FLAG, TAG_RKP_FLAG).unwrap_or(false);
    let ids_ok = kph_verify(&api, UUID_ATTESTATION_IDS, TAG_ATTESTATION_IDS).unwrap_or(false);

    if !rkp_ok {
        bail!("post-import verification failed: rkp_flag not found in secure storage");
    }
    if !ids_ok {
        bail!("post-import verification failed: attestation_ids not found in secure storage");
    }

    eprintln!("trustkernel: both targets verified");

    Ok(crate::SessionInfo {
        loaded_library: Some(api.lib_path),
        ta_api_version: None,
        ta_version: None,
        response_hex: None,
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
    Ok(())
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
        assert!(payload.contains("BRAND=Xelex"));
        assert!(payload.contains("MANUFACTURER=Zinwa"));
    }

    #[test]
    fn attestation_ids_field_order_is_correct() {
        let payload = build_attestation_ids_payload(&test_profile());
        let brand_pos = payload.find("BRAND=").unwrap();
        let device_pos = payload.find("DEVICE=").unwrap();
        let product_pos = payload.find("PRODUCT=").unwrap();
        let manufacturer_pos = payload.find("MANUFACTURER=").unwrap();
        let model_pos = payload.find("MODEL=").unwrap();
        let serial_pos = payload.find("SERIAL=").unwrap();
        // The TrustKernel TA requires this exact positional order.
        assert!(brand_pos < device_pos);
        assert!(device_pos < product_pos);
        assert!(product_pos < manufacturer_pos);
        assert!(manufacturer_pos < model_pos);
        assert!(model_pos < serial_pos);
    }

    #[test]
    fn rkp_flag_payload_format() {
        let profile = test_profile();
        let payload = build_rkp_flag_payload(&profile);
        // On a non-Android host, the platform prop returns empty.
        assert!(payload.starts_with("BRAND=Xelex"));
        assert!(payload.contains("MODEL=Q25"));
        assert!(payload.contains("PRODUCT=Q25_EEA"));
    }

    #[test]
    fn context_buffer_initialization() {
        let mut context = [0u8; KPH_CONTEXT_BUF_SIZE];
        context[..4].copy_from_slice(&KPH_CONTEXT_MAGIC.to_le_bytes());
        // First four bytes: 42 as LE u32 = [0x2A, 0x00, 0x00, 0x00].
        assert_eq!(context[0], 0x2A);
        assert_eq!(context[1], 0x00);
        assert_eq!(context[2], 0x00);
        assert_eq!(context[3], 0x00);
        // Remainder must be zero.
        assert!(context[4..].iter().all(|&b| b == 0));
    }
}
