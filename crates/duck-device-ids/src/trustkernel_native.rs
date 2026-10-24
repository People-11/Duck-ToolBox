//! The UUID-addressed ABI observed in the original KPH call sites. Only a disposable
//! worker loads vendor code; legacy numeric-ID symbols are not interchangeable.
use crate::trustkernel_worker::{Call, Reply};
use anyhow::{Context, Result};
use libloading::Library;
use std::{
    ffi::{CString, c_char, c_int},
    path::Path,
};

// Seven arguments: UUID, tag, context, context length, payload, payload length, storage.
type ImportByUuid = unsafe extern "C" fn(
    *const c_char,
    *const c_char,
    *const u8,
    u32,
    *const u8,
    u32,
    u32,
) -> c_int;
type VerifyByUuid = unsafe extern "C" fn(*const c_char, *const c_char) -> c_int;
type TaVersion = unsafe extern "C" fn(*mut c_char, u32) -> c_int;

pub(crate) fn call(path: &Path, call: &Call) -> Result<Reply> {
    call.validate()?;
    // SAFETY: This entry is invoked only inside the isolated worker (or with a synthetic
    // library in tests). The handle outlives each resolved function and synchronous call.
    let library = unsafe { Library::new(path) }.context("load TrustKernel libkphproxy.so")?;
    let mut version = None;
    let status = match call {
        Call::Version => {
            // TA version is optional; absence must not prevent a supported import ABI.
            if let Ok(function) = unsafe { library.get::<TaVersion>(b"kph_ta_version\0") } {
                let mut buffer = [0_u8; 32];
                let status = unsafe { function(buffer.as_mut_ptr().cast(), buffer.len() as u32) };
                if status == 0
                    && let Some(end) = buffer.iter().position(|&byte| byte == 0)
                {
                    let value = String::from_utf8_lossy(&buffer[..end]).trim().to_owned();
                    if !value.is_empty() {
                        version = Some(value);
                    }
                }
                status
            } else {
                0
            }
        }
        Call::Import { item } => {
            // Resolve both required entry points before the first storage mutation.
            let function = unsafe { library.get::<ImportByUuid>(b"kph_import_ta_data_by_uuid\0") }
                .context("resolve UUID-addressed KPH import (seven-argument ABI)")?;
            let _verify = unsafe { library.get::<VerifyByUuid>(b"kph_verify_data_by_uuid\0") }
                .context("resolve UUID-addressed KPH verification")?;
            let uuid = CString::new(item.uuid.as_str())?;
            let tag = CString::new(item.tag.as_str())?;
            let mut context = [0_u8; 128];
            context[..4].copy_from_slice(&item.permission.to_le_bytes());
            // SAFETY: Inputs stay alive for the call, lengths match their buffers, and
            // this exact argument order is the UUID call site recovered from KPH.
            unsafe {
                function(
                    uuid.as_ptr(),
                    tag.as_ptr(),
                    context.as_ptr(),
                    context.len() as u32,
                    item.payload.as_ptr(),
                    u32::try_from(item.payload.len())?,
                    item.flags,
                )
            }
        }
        Call::Verify { uuid, tag } => {
            let function = unsafe { library.get::<VerifyByUuid>(b"kph_verify_data_by_uuid\0") }
                .context("resolve UUID-addressed KPH verification")?;
            let uuid = CString::new(uuid.as_str())?;
            let tag = CString::new(tag.as_str())?;
            unsafe { function(uuid.as_ptr(), tag.as_ptr()) }
        }
    };
    Ok(Reply::Returned {
        library: path.to_owned(),
        status,
        version,
    })
}
