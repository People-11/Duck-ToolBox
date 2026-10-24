use super::*;

#[test]
fn issuance_policy_distinguishes_vendor_evidence() {
    for (state, expected) in [
        (State::Qualcomm, None),
        (State::MediaTek, None),
        (State::Other, Some("qualcomm_soc_required")),
        (State::Unknown, Some("soc_vendor_unknown")),
        (State::Conflicting, Some("soc_vendor_conflict")),
    ] {
        assert_eq!(
            blocker(&Detection {
                state,
                model: String::new(),
                evidence: vec![]
            }),
            expected
        );
    }
}
