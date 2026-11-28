//! A query a plugin runs through the host may take 5 seconds, and one an
//! agent types with `board query` keeps 1 second. Each refusal names the
//! limit that applied, and an inner query past 1 second shows in the trace
//! under its own name.

use super::*;

/// A statement that never finishes, so it runs to whichever limit applies.
const ENDLESS: &str = "WITH RECURSIVE c(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM c) \
                       SELECT count(*) FROM c";

#[test]
fn a_plugin_query_through_the_host_stops_at_5s_and_shows_as_slow_and_board_query_stops_at_1s() {
    let scenario = board(&[("fixture", true)], &["fixture"], Holds::Same);
    let typed = open(&scenario)
        .query(ENDLESS, 1, None, None)
        .unwrap_err()
        .refusal();
    assert_eq!(typed.code.as_str(), Code::QueryTimeout.as_str());
    assert_eq!(typed.message, "the query ran longer than 1s");

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
    let trace = board.take_trace();
    let slow = (trace.phases.iter())
        .find(|phase| phase.name == "query.slow")
        .expect("the inner query shows as slow");
    assert!(slow.took >= Duration::from_secs(5), "{slow:?}");
}
