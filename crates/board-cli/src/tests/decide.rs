//! `board decide` shows the owner the text or record its file holds, up to
//! `MAX_DECIDE_BYTES`.

use board_model::MAX_DECIDE_BYTES;

use super::*;

#[test]
fn decide_shows_text_on_stdin_at_the_limit_and_refuses_one_byte_more() {
    let scenario = board();
    let decide = |bytes: usize| {
        let arguments = [
            "--json",
            "decide",
            "--op",
            "review.comment",
            "--path",
            "/projects/p",
            "--text",
            "-",
        ];
        board_with(
            scenario.workdir(),
            None,
            &arguments,
            "a".repeat(bytes).as_bytes(),
        )
    };
    let over = decide(MAX_DECIDE_BYTES + 1);
    assert_eq!(over.code, 1, "{}", text(&over.stderr));
    assert_eq!(json(&over)["code"], "board::json::too_large");
    let at = decide(MAX_DECIDE_BYTES);
    assert_eq!(at.code, 0, "{}", text(&at.stderr));
    assert_eq!(json(&at)["data"]["op"], "review.comment");
}
