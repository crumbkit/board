//! What signing's state asks of a change's sign-offs: while signing is on,
//! only the owner's Touch ID approval of `changes.approve` signs off a plan,
//! data or mockups that it lists (docs/architecture/writes/signing/optional.md).

use std::collections::BTreeSet;

use board_model::{Act, Need, Signing};

use super::{Approval, Observed, UNDERSTANDING_TYPE};

impl Observed {
    /// Whether signing lists `act`, so that only the owner's Touch ID
    /// approval of `changes.approve` for the current plan signs it off.
    pub(crate) fn needs_signature(&self, act: Act) -> bool {
        self.signed.contains(&act)
    }
}

/// The sign-offs of a change that need the owner's signature now, as
/// signing's state says: those it lists while it is on.
pub(crate) fn signed_acts(signing: &Signing) -> BTreeSet<Act> {
    ([Act::Plan, Act::Data, Act::Mockup].into_iter())
        .filter(|act| signing.need(*act) != Need::Answer)
        .collect()
}

/// Why a plan is not approved while signing lists plans: no Touch ID
/// approval for it, which `unread` says Board could not read when it says
/// anything.
pub(super) fn plan_unsigned(path: &str, unread: &str) -> String {
    format!(
        "signing lists plans, so only the owner's Touch ID approval of changes.approve for the \
         current plan approves it; a decision or an answer does not{unread}; run \
         `board changes approve --path {path}`"
    )
}

/// Why a change's data is not signed off while signing lists data
/// sign-offs, or none when the owner's Touch ID approval of the current
/// plan stands beside an attached understanding check, as `understood`
/// says.
pub(super) fn data_unsigned(observed: &Observed, understood: bool) -> Option<String> {
    let signed = observed.approval == Approval::Signed && understood;
    (observed.needs_signature(Act::Data) && !signed).then(|| {
        format!(
            "signing lists data sign-offs, so only the owner's Touch ID approval of \
             changes.approve for the current plan signs off this change's data, beside an \
             attached {UNDERSTANDING_TYPE} of it; a decision or an answer does not; run \
             `board changes approve --path {}`",
            observed.path
        )
    })
}
