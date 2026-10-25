//! The query the Claude Code effects mod lists pending effects with
//! (plugins/effects/claude-code/hooks/query.ts), run against a real board.

use super::*;

const QUERY_TS: &str = include_str!("../../../../plugins/effects/claude-code/hooks/query.ts");

/// The text of `PENDING_SQL`: after its opening backtick, up to the next.
fn pending_sql() -> &'static str {
    let start = QUERY_TS.find("PENDING_SQL = `").unwrap() + "PENDING_SQL = `".len();
    let length = QUERY_TS[start..].find('`').unwrap();
    &QUERY_TS[start..start + length]
}

fn effect(path: &str, content: &Value) -> String {
    let mut effect = json!({"$type": "board/effect/v1", "board": "/b", "plugin": "notes",
        "op": "put", "path": path, "authority": "ask", "digest": format!("sha256:{}", "0".repeat(64))});
    effect
        .as_object_mut()
        .unwrap()
        .extend(content.as_object().unwrap().clone());
    effect.to_string()
}

#[test]
fn the_effects_mod_lists_each_pending_effect_no_seen_record_answers_with_the_start_of_its_content()
{
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|digit| digit.repeat(64));
    let scenario = board();
    let long = "x".repeat(2500);
    let seen = json!({"$type": "board/seen/v1", "effect": format!("/effects/pending/{c}"), "answer": "approve"});
    scenario
        .write(
            &format!("effects/pending/{a}.json"),
            Item::File(&effect(
                "/notes/a.md",
                &json!({"text": "one\ntwo\n", "prior": "f00d"}),
            )),
        )
        .write(
            &format!("effects/pending/{b}.json"),
            Item::File(&effect(
                "/tasks/b",
                &json!({"record": {"$type": "lr/task/v1", "title": long}}),
            )),
        )
        .write(
            &format!("effects/pending/{c}.json"),
            Item::File(&effect("/notes/c.md", &json!({"text": "c"}))),
        )
        .write(
            &format!("effects/seen/{c}.json"),
            Item::File(&seen.to_string()),
        )
        // A record of another type where a seen record would be answers nothing.
        .write(
            &format!("effects/pending/{d}.json"),
            Item::File(&effect("/notes/d.md", &json!({"text": "d"}))),
        )
        .write(
            &format!("effects/seen/{d}.json"),
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        );
    let ran = board_in(
        scenario.workdir(),
        &["--json", "query", pending_sql(), "--limit", "100"],
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let report = json(&ran);
    let columns = [
        "id", "board", "plugin", "op", "path", "replaces", "head", "bytes", "lines",
    ];
    assert_eq!(report["summary"]["columns"], json!(columns));
    let record = format!(r#"{{"$type":"lr/task/v1","title":"{long}"}}"#);
    assert_eq!(
        report["data"],
        json!([
            [
                a,
                "/b",
                "notes",
                "put",
                "/notes/a.md",
                1,
                "one\ntwo\n",
                8,
                2
            ],
            [
                b,
                "/b",
                "notes",
                "put",
                "/tasks/b",
                0,
                &record[..2000],
                record.len(),
                1
            ],
            [d, "/b", "notes", "put", "/notes/d.md", 0, "d", 1, 1],
        ])
    );
}
