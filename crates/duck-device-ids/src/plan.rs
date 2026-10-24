//! Backend-specific requests shared by dry runs, execution and reporting.
use anyhow::Result;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

use crate::{Backend, DeviceIdsError, DeviceIdsProfile, ProvisionedId, command, spec};

#[derive(Debug, Serialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub(crate) enum Request {
    Qseecom {
        #[serde(serialize_with = "hex_bytes")]
        command: Vec<u8>,
    },
    Trustkernel {
        items: Vec<DataItem>,
    },
}

#[derive(Debug, Serialize)]
pub(crate) struct DataItem {
    pub uuid: &'static str,
    pub tag: &'static str,
    #[serde(serialize_with = "hex_bytes")]
    pub payload: Vec<u8>,
    pub permission: u32,
    pub flags: u32,
}

#[derive(Debug, Serialize)]
pub(crate) struct Plan {
    pub request: Request,
    pub ids: Vec<ProvisionedId>,
    pub omitted_ids: Vec<String>,
}

impl Plan {
    pub fn build(
        profile: &DeviceIdsProfile,
        backend: Backend,
        properties: &BTreeMap<String, String>,
    ) -> Result<Self> {
        let ids = spec::collect_ids(profile, backend)?;
        if backend == Backend::Qseecom {
            return Ok(Self {
                request: Request::Qseecom {
                    command: command::build(&ids)?,
                },
                ids: ids
                    .into_iter()
                    .map(|id| ProvisionedId {
                        label: id.label.into(),
                        value: id.value,
                    })
                    .collect(),
                omitted_ids: vec![],
            });
        }
        if !matches!(profile.ta_name.trim(), "" | "keymaster" | "keymaster64")
            || !matches!(
                profile.ta_path.trim(),
                "" | "/vendor/app/t6" | "/vendor/firmware_mnt/image"
            )
        {
            return Err(DeviceIdsError::Unsupported("TA location overrides apply only to QSEECom; TrustKernel uses the reported target UUIDs").into());
        }
        let mut omitted_ids = vec![];
        let ids: Vec<_> = ids
            .into_iter()
            .filter_map(|id| {
                if matches!(id.label, "MEID" | "MEID2") {
                    omitted_ids.push(id.label.into());
                    None
                } else {
                    Some(ProvisionedId {
                        label: id.label.into(),
                        value: id.value,
                    })
                }
            })
            .collect();
        for id in &ids {
            validate_value(&id.label, &id.value)?;
        }
        let value = |label: &str| {
            ids.iter()
                .find(|id| id.label == label)
                .map(|id| id.value.as_str())
                .unwrap_or("")
        };
        let platform = [
            "ro.vendor.mediatek.platform",
            "ro.hardware.mediatek.platform",
            "ro.board.platform",
        ]
        .iter()
        .find_map(|key| {
            properties
                .get(*key)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or("");
        validate_value("PLATFORM", platform)?;
        let rkp = format!(
            "BRAND={} MODEL={} PLATFORM={} PRODUCT={}",
            value("BRAND"),
            value("MODEL"),
            platform,
            value("PRODUCT")
        );
        let payload = [
            "BRAND",
            "DEVICE",
            "PRODUCT",
            "MANUFACTURER",
            "MODEL",
            "SERIAL",
            "IMEI",
            "IMEI2",
        ]
        .iter()
        .filter_map(|label| ids.iter().find(|id| id.label == *label))
        .map(|id| format!("{}={}", id.label, id.value))
        .collect::<Vec<_>>()
        .join(" ");
        Ok(Self {
            request: Request::Trustkernel {
                items: vec![
                    item("b46325e6-5c90-8252-2ead-a8e32e5180d6", "rkp_flag", rkp),
                    item(
                        "2ea702fa-17bc-4752-b3ad-b2871a772347",
                        "attestation_ids",
                        payload,
                    ),
                ],
            },
            ids,
            omitted_ids,
        })
    }
}

fn item(uuid: &'static str, tag: &'static str, payload: String) -> DataItem {
    let mut payload = payload.into_bytes();
    payload.push(0);
    DataItem {
        uuid,
        tag,
        payload,
        permission: 42,
        flags: 0,
    }
}

fn validate_value(label: &str, value: &str) -> Result<()> {
    if value.len() > 1024
        || value.chars().any(char::is_control)
        || value.split_whitespace().any(|word| word.contains('='))
    {
        return Err(DeviceIdsError::InvalidValue(label.to_owned()).into());
    }
    Ok(())
}

fn hex_bytes<S: Serializer>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&hex::encode(bytes))
}
