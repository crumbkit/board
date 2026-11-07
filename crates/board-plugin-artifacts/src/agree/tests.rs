//! `agree` agrees any artifact that is not superseded.

use super::*;
use crate::set::tests::held;

/// An artifact whose state this Board does not know can be agreed as a
/// draft one can, and a superseded twin is refused.
#[test]
fn a_state_this_board_does_not_know_is_agreeable_like_draft_and_a_superseded_twin_is_refused() {
    for state in ["draft", "parked"] {
        assert!(agreeable("/artifacts/A", &held(state)).is_ok(), "{state}");
    }
    let refused = agreeable("/artifacts/A", &held("superseded")).unwrap_err();
    assert_eq!(refused.code, "artifacts::state::settled");
}
