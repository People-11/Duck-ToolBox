pub(crate) use duck_platform::soc::{Detection, State, detect};

pub(crate) fn blocker(detection: &Detection) -> Option<&'static str> {
    match detection.state {
        State::Qualcomm | State::MediaTek => None,
        State::Other => Some("qualcomm_soc_required"),
        State::Unknown => Some("soc_vendor_unknown"),
        State::Conflicting => Some("soc_vendor_conflict"),
    }
}

#[cfg(test)]
#[path = "soc_tests.rs"]
mod tests;
