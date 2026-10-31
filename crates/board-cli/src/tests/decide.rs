//! `board decide` shows the owner the text or record its file holds, up to
//! one mebibyte.

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
    // Literals, so that the test also pins the limit itself.
    let over = decide(1_048_577);
    assert_eq!(over.code, 1, "{}", text(&over.stderr));
    assert_eq!(json(&over)["code"], "board::json::too_large");
    let at = decide(1_048_576);
    assert_eq!(at.code, 0, "{}", text(&at.stderr));
    assert_eq!(json(&at)["data"]["op"], "review.comment");
}
