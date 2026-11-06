//! `set` gives a state only to an artifact that is not settled.

use serde_json::json;

use super::*;

/// An artifact record in `state`.
pub(crate) fn held(state: &str) -> ArtifactRecord {
    let record = json!({
        "$type": "artifacts/artifact/v1",
        "url": "https://claude.ai/artifact/A",
        "account": {"person": "owner@example.com"},
        "first_seen": "2026-10-31T18:20:00Z",
        "state": state,
    });
    ArtifactRecord::deserialize(&record).unwrap()
}

/// An artifact whose state a newer Board added takes a state as a shown
/// one does, and an agreed twin is refused.
#[test]
fn a_state_a_newer_board_added_is_settable_like_shown_and_an_agreed_twin_is_refused() {
    for state in ["shown", "parked"] {
        assert!(settable("/artifacts/A", &held(state)).is_ok(), "{state}");
    }
    let refused = settable("/artifacts/A", &held("agreed")).unwrap_err();
    assert_eq!(refused.code, "artifacts::state::settled");
}
