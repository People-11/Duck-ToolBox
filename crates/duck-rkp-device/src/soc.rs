use std::{collections::BTreeMap, fs::File, io::Read};

use duck_core::Sysroot;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum State {
    Qualcomm,
    MediaTek,
    Other,
    Unknown,
    Conflicting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Vendor {
    Qualcomm,
    MediaTek,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Evidence {
    pub source: &'static str,
    pub value: String,
    pub vendor: Option<Vendor>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Detection {
    pub state: State,
    pub model: String,
    pub evidence: Vec<Evidence>,
}

impl Detection {
    pub fn blocker(&self) -> Option<&'static str> {
        match self.state {
            State::Qualcomm | State::MediaTek => None,
            State::Other => Some("qualcomm_soc_required"),
            State::Unknown => Some("soc_vendor_unknown"),
            State::Conflicting => Some("soc_vendor_conflict"),
        }
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Manufacturer,
    Model,
    Hardware,
    Family,
}

pub(crate) fn detect(properties: &BTreeMap<String, String>, sysroot: &Sysroot) -> Detection {
    let mut evidence = Vec::new();
    for (source, kind) in [
        ("ro.soc.manufacturer", Kind::Manufacturer),
        ("ro.vendor.soc.manufacturer", Kind::Manufacturer),
        ("ro.soc.model", Kind::Model),
        ("ro.vendor.soc.model", Kind::Model),
        ("ro.vendor.qti.soc_model", Kind::Model),
        ("ro.hardware", Kind::Hardware),
        ("ro.boot.hardware", Kind::Hardware),
        ("ro.board.platform", Kind::Hardware),
        ("ro.product.board", Kind::Hardware),
        ("ro.hardware.chipname", Kind::Model),
        ("ro.vendor.mediatek.platform", Kind::Hardware),
    ] {
        if let Some(value) = properties.get(source) {
            add(&mut evidence, source, value, kind);
        }
    }
    for (source, kind) in [
        ("/sys/devices/soc0/family", Kind::Family),
        ("/sys/devices/soc0/machine", Kind::Model),
        ("/sys/devices/system/soc/soc0/family", Kind::Family),
        ("/sys/devices/system/soc/soc0/machine", Kind::Model),
    ] {
        // Sysfs attributes can report a page-sized metadata length. Bound the actual
        // read instead; unreadable or oversized attributes provide no evidence.
        if let Ok(file) = File::open(sysroot.path(source)) {
            let mut value = String::new();
            if file.take(257).read_to_string(&mut value).is_ok() && value.len() <= 256 {
                add(&mut evidence, source, &value, kind);
            }
        }
    }
    let qualcomm = evidence
        .iter()
        .any(|item| item.vendor == Some(Vendor::Qualcomm));
    let mediatek = evidence
        .iter()
        .any(|item| item.vendor == Some(Vendor::MediaTek));
    let other = evidence
        .iter()
        .any(|item| item.vendor == Some(Vendor::Other));
    let vendor_count = [qualcomm, mediatek, other].iter().filter(|&&v| v).count();
    let state = if vendor_count > 1 {
        State::Conflicting
    } else if qualcomm {
        State::Qualcomm
    } else if mediatek {
        State::MediaTek
    } else if other {
        State::Other
    } else {
        State::Unknown
    };
    let model = evidence
        .iter()
        .find(|item| item.source.ends_with("model") || item.source.ends_with("machine"))
        .map(|item| item.value.clone())
        .unwrap_or_default();
    Detection {
        state,
        model,
        evidence,
    }
}

fn add(evidence: &mut Vec<Evidence>, source: &'static str, value: &str, kind: Kind) {
    let value = value.trim();
    if value.is_empty()
        || matches!(
            value.to_ascii_lowercase().as_str(),
            "unknown" | "null" | "none" | "n/a"
        )
    {
        return;
    }
    let vendor = match kind {
        Kind::Manufacturer => manufacturer(value),
        Kind::Model => {
            if qualcomm_model(value) {
                Some(Vendor::Qualcomm)
            } else if mediatek_model(value) {
                Some(Vendor::MediaTek)
            } else {
                None
            }
        }
        Kind::Hardware => manufacturer(value).or_else(|| {
            if qualcomm_model(value) {
                Some(Vendor::Qualcomm)
            } else if mediatek_model(value) {
                Some(Vendor::MediaTek)
            } else {
                None
            }
        }),
        Kind::Family => {
            if value.eq_ignore_ascii_case("snapdragon") {
                Some(Vendor::Qualcomm)
            } else {
                manufacturer(value)
            }
        }
    };
    evidence.push(Evidence {
        source,
        value: value.to_owned(),
        vendor,
    });
}

fn manufacturer(value: &str) -> Option<Vendor> {
    let normalized = value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    match normalized.as_str() {
        "qti"
        | "qcom"
        | "qualcomm"
        | "qualcomm inc"
        | "qualcomm incorporated"
        | "qualcomm technologies"
        | "qualcomm technologies inc"
        | "qualcomm technologies incorporated"
        | "qualcomm technologies ltd" => Some(Vendor::Qualcomm),
        "mediatek"
        | "mediatek inc"
        | "mediatek incorporated"
        | "mtk" => Some(Vendor::MediaTek),
        "samsung"
        | "samsung electronics"
        | "samsung electronics co ltd"
        | "exynos"
        | "google"
        | "google inc"
        | "google tensor"
        | "tensor"
        | "hisilicon"
        | "hisilicon technologies co ltd"
        | "kirin"
        | "unisoc"
        | "spreadtrum"
        | "sprd"
        | "rockchip"
        | "amlogic"
        | "nvidia"
        | "intel"
        | "amd"
        | "apple"
        | "broadcom" => Some(Vendor::Other),
        _ => None,
    }
}

fn qualcomm_model(value: &str) -> bool {
    let model = value.to_ascii_uppercase();
    let (part, suffix) = model.split_once('-').unwrap_or((&model, ""));
    if suffix.len() > 4 || !suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return false;
    }
    ["MSM", "APQ", "SDM", "SM", "QCM", "QCS"]
        .iter()
        .any(|prefix| {
            let Some(number) = part.strip_prefix(prefix) else {
                return false;
            };
            let digits = number.bytes().take_while(u8::is_ascii_digit).count();
            let expected = if matches!(*prefix, "SM" | "QCM" | "QCS") {
                4..=4
            } else {
                3..=4
            };
            expected.contains(&digits)
                && number.len() - digits <= 2
                && number[digits..]
                    .bytes()
                    .all(|byte| byte.is_ascii_uppercase())
        })
}

/// Matches MediaTek SoC model strings (MT6878, MT6789, MT6985, etc.).
/// Format: "MT" followed by 4 digits, with an optional 1-2 character alpha suffix.
fn mediatek_model(value: &str) -> bool {
    let model = value.to_ascii_uppercase();
    let (part, suffix) = model.split_once('/').unwrap_or((&model, ""));
    if !suffix.is_empty() {
        return false;
    }
    let Some(number) = part.strip_prefix("MT") else {
        return false;
    };
    let digits = number.bytes().take_while(u8::is_ascii_digit).count();
    (4..=4).contains(&digits)
        && number.len() - digits <= 2
        && number[digits..]
            .bytes()
            .all(|byte| byte.is_ascii_uppercase())
}

#[cfg(test)]
#[path = "soc_tests.rs"]
mod tests;
