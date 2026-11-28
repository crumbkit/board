//! A query a plugin runs through the host may take 5 seconds, and one an
//! agent types with `board query` keeps 1 second. Each refusal names the
//! limit that applied, and its hint fits who wrote the statement. An inner
//! query that finishes after more than 1 second shows in the trace under
//! its own name.

use std::time::Duration;

use super::*;
use crate::query::{INNER_QUERY_TIME, INNER_TIMEOUT_HINT, TYPED_QUERY_TIME, slow};

/// A statement that never finishes, so it runs to whichever limit applies.
const ENDLESS: &str = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c) \
                       SELECT count(*) FROM c";

#[test]
fn a_plugin_query_through_the_host_stops_at_5s_with_a_hint_to_run_again_and_board_query_stops_at_1s_with_a_hint_to_narrow()
 {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let typed = open(&scenario)
        .query(ENDLESS, 1, None, None)
        .unwrap_err()
        .refusal();
    assert_eq!(typed.code.as_str(), Code::QueryTimeout.as_str());
    assert_eq!(typed.message, "the query ran longer than 1s");
    let narrow = typed.hint.as_deref().unwrap();
    assert!(narrow.contains("WHERE"), "{narrow}");
    assert!(!narrow.contains("second"), "{narrow}");

    let mut board = open(&scenario);
    board.build_in(built(vec![querying(ENDLESS)]));
    let refusal = call_base(&board, &Plugins::default())
        .unwrap_err()
        .refusal();
    assert_eq!(refusal.fields["code"], json!(Code::QueryTimeout.as_str()));
    assert!(
        refusal.message.ends_with("the query ran longer than 5s"),
        "{}",
        refusal.message
    );
    assert_eq!(refusal.hint.as_deref(), Some(INNER_TIMEOUT_HINT));
    // A statement stopped at its limit did not finish slowly.
    let trace = board.take_trace();
    assert!(
        !trace.phases.iter().any(|phase| phase.name == "query.slow"),
        "{trace:?}"
    );
}

#[test]
fn only_a_statement_allowed_more_than_1s_that_finishes_after_more_than_1s_is_slow() {
    let past = TYPED_QUERY_TIME + Duration::from_millis(1);
    assert!(slow(INNER_QUERY_TIME, past, false));
    assert!(!slow(INNER_QUERY_TIME, past, true), "it timed out");
    assert!(!slow(INNER_QUERY_TIME, TYPED_QUERY_TIME, false));
    assert!(!slow(TYPED_QUERY_TIME, past, false), "an agent typed it");
}
