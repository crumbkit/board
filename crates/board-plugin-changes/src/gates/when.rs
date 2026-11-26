//! The tests of a check's `when` that need no plan text: its route and the
//! plan's typed answers, which the gate and `changes.owed` both read.

use crate::flow;
use crate::plan::answers::Answers;
use crate::records::Route;

/// Whether the check's `when` fails without the plan's text: on the
/// change's `route`, or on the plan's `answers` when they are given.
///
/// [`applies`](super::applies) reads a `when`'s route first, then its
/// section patterns, then its answers, and a pattern it cannot read makes
/// the check missing, so a `when` with section patterns is left to the gate
/// whatever the answers. Both read the route and the answers through [`off_route`] and
/// [`unanswered`], so a caller that skips a check on this answer judges it
/// as the gate does.
pub(crate) fn fails_unread(
    each: &flow::Check,
    route: Option<Route>,
    answers: Option<Answers>,
) -> bool {
    let Some(when) = &each.when else {
        return false;
    };
    if off_route(when, route) {
        return true;
    }
    if when.section.is_some() || when.section_not.is_some() {
        return false;
    }
    answers.is_some_and(|answers| unanswered(when, answers))
}

/// Whether the `when` names routes and the change's `route` is not one of
/// them.
pub(super) fn off_route(when: &flow::When, route: Option<Route>) -> bool {
    (when.route.as_ref()).is_some_and(|routes| !route.is_some_and(|route| routes.contains(&route)))
}

/// Whether the plan's `answers` fail the `when`: one its `answers` names
/// differs (an answer the plan leaves out counts as meeting it), or the
/// plan gives every answer its `answers_not` names.
pub(super) fn unanswered(when: &flow::When, answers: Answers) -> bool {
    when.answers.is_some_and(|wanted| !answers.meet(wanted))
        || when.answers_not.is_some_and(|unless| hold(answers, unless))
}

/// Whether the plan gives each answer `wanted` names, equal to it; an
/// answer the plan leaves out does not hold.
fn hold(given: Answers, wanted: Answers) -> bool {
    (wanted.ux.is_none() || given.ux == wanted.ux)
        && (wanted.data.is_none() || given.data == wanted.data)
        && (wanted.lifecycle.is_none() || given.lifecycle == wanted.lifecycle)
}
