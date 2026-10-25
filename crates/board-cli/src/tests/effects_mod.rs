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
    let [a, b, c, d, empty] = ["a", "b", "c", "d", "e"].map(|digit| digit.repeat(64));
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
        )
        .write(
            &format!("effects/pending/{empty}.json"),
            Item::File(&effect("/notes/e.md", &json!({"text": ""}))),
        );
    let ran = board_in(
        scenario.workdir(),
        &["--json", "query", pending_sql(), "--limit", "100"],
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let report = json(&ran);
    let columns = [
        "id", "board", "plugin", "op", "path", "replaces", "head", "bytes", "nul", "lines",
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
                0,
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
                0,
                1
            ],
            [d, "/b", "notes", "put", "/notes/d.md", 0, "d", 1, 0, 1],
            [empty, "/b", "notes", "put", "/notes/e.md", 0, "", 0, 0, 0],
        ])
    );
}

/// The mod's one-effect query: `pendingSql(id)` in effects.ts narrows the
/// list's `LIKE` to the effect's own path.
fn one_sql(id: &str) -> String {
    let every = "logical LIKE '/effects/pending/%'";
    assert_eq!(pending_sql().matches(every).count(), 1);
    pending_sql().replace(every, &format!("logical = '/effects/pending/{id}'"))
}

#[test]
fn the_effects_mods_one_effect_query_finds_that_effect_while_no_seen_record_answers_it() {
    let [a, c] = ["a", "c"].map(|digit| digit.repeat(64));
    let scenario = board();
    let seen = json!({"$type": "board/seen/v1", "effect": format!("/effects/pending/{c}"), "answer": "refuse"});
    scenario
        .write(
            &format!("effects/pending/{a}.json"),
            Item::File(&effect("/notes/a.md", &json!({"text": "a\n"}))),
        )
        .write(
            &format!("effects/pending/{c}.json"),
            Item::File(&effect("/notes/c.md", &json!({"text": "c"}))),
        )
        .write(
            &format!("effects/seen/{c}.json"),
            Item::File(&seen.to_string()),
        );
    let ids = |id: &str| {
        let ran = board_in(scenario.workdir(), &["--json", "query", &one_sql(id)]);
        assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
        json(&ran)["data"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row[0].clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), [json!(a)]);
    assert!(ids(&c).is_empty());
}

#[test]
fn the_effects_mod_query_says_when_content_holds_a_nul_before_which_its_head_stops() {
    let id = "f".repeat(64);
    let scenario = board();
    scenario.write(
        &format!("effects/pending/{id}.json"),
        Item::File(&effect(
            "/notes/f.md",
            &json!({"text": "harmless config\n\u{0}\nrm -rf /\nmore\n"}),
        )),
    );
    let ran = board_in(
        scenario.workdir(),
        &["--json", "query", pending_sql(), "--limit", "100"],
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    // SQLite's text functions stop at the NUL, so `head` and `lines` cover
    // only "harmless config\n" while `bytes` counts all 32 bytes.
    assert_eq!(
        json(&ran)["data"],
        json!([[
            id,
            "/b",
            "notes",
            "put",
            "/notes/f.md",
            0,
            "harmless config\n",
            32,
            1,
            1
        ]])
    );
}
