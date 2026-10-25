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

/// The mod's one-effect query: `pendingSql(id)` in effects.ts narrows the
/// list's `LIKE` to the effect's own path.
fn one_sql(id: &str) -> String {
    let every = "logical LIKE '/effects/pending/%'";
    assert_eq!(pending_sql().matches(every).count(), 1);
    pending_sql().replace(every, &format!("logical = '/effects/pending/{id}'"))
}

/// Writes a pending put of `path` at `id`, with `content`'s fields added.
fn pend(scenario: &Scenario, id: &str, path: &str, content: &Value) {
    let mut effect = json!({"$type": "board/effect/v1", "board": "/b", "plugin": "notes",
        "op": "put", "path": path, "authority": "ask", "digest": format!("sha256:{}", "0".repeat(64))});
    effect
        .as_object_mut()
        .unwrap()
        .extend(content.as_object().unwrap().clone());
    scenario.write(
        &format!("effects/pending/{id}.json"),
        Item::File(&effect.to_string()),
    );
}

/// Writes `record` where the seen record of `id` goes.
fn see(scenario: &Scenario, id: &str, record: &Value) {
    scenario.write(
        &format!("effects/seen/{id}.json"),
        Item::File(&record.to_string()),
    );
}

/// A seen record of `id` with `answer`.
fn seen(id: &str, answer: &str) -> Value {
    json!({"$type": "board/seen/v1", "effect": format!("/effects/pending/{id}"), "answer": answer})
}

/// What `sql` returns in `scenario`: its columns and its rows.
fn query(scenario: &Scenario, sql: &str) -> (Value, Value) {
    let ran = board_in(
        scenario.workdir(),
        &["--json", "query", sql, "--limit", "100"],
    );
    assert_eq!(ran.code, 0, "{}", text(&ran.stdout));
    let report = json(&ran);
    (report["summary"]["columns"].clone(), report["data"].clone())
}

/// A row as the query returns it for an effect `pend` wrote.
fn row(id: &str, path: &str, replaces: u8, head: &str, bytes: usize, nul: u8, lines: u8) -> Value {
    json!([
        id, "/b", "notes", "put", path, replaces, head, bytes, nul, lines
    ])
}

#[test]
fn the_effects_mod_lists_each_pending_effect_no_seen_record_answers_with_the_start_of_its_content()
{
    let [a, b, c, d, empty, malformed] =
        ["a", "b", "c", "d", "e", "f"].map(|digit| digit.repeat(64));
    let [no_effect, extra_key, bad_reason, reasoned] =
        ["1", "2", "3", "4"].map(|digit| digit.repeat(64));
    let scenario = board();
    let long = "x".repeat(2500);
    pend(
        &scenario,
        &a,
        "/notes/a.md",
        &json!({"text": "one\ntwo\n", "prior": "f00d"}),
    );
    pend(
        &scenario,
        &b,
        "/tasks/b",
        &json!({"record": {"$type": "lr/task/v1", "title": long}}),
    );
    pend(&scenario, &c, "/notes/c.md", &json!({"text": "c"}));
    see(&scenario, &c, &seen(&c, "approve"));
    // A record of another type where a seen record would be answers nothing.
    pend(&scenario, &d, "/notes/d.md", &json!({"text": "d"}));
    see(&scenario, &d, &json!({"$type": "lr/project/v1"}));
    pend(&scenario, &empty, "/notes/e.md", &json!({"text": ""}));
    // Nor does a seen record whose answer Board does not read.
    pend(&scenario, &malformed, "/notes/f.md", &json!({"text": "f"}));
    see(&scenario, &malformed, &seen(&malformed, "maybe"));
    let mut no_effect_seen = seen(&no_effect, "approve");
    no_effect_seen.as_object_mut().unwrap().remove("effect");
    let mut extra_key_seen = seen(&extra_key, "approve");
    extra_key_seen["note"] = json!("x");
    let mut bad_reason_seen = seen(&bad_reason, "refuse");
    bad_reason_seen["reason"] = json!(7);
    // A string reason is an answer, so that effect is not listed.
    let mut reasoned_seen = seen(&reasoned, "refuse");
    reasoned_seen["reason"] = json!("not now");
    for (id, record) in [
        (&no_effect, no_effect_seen),
        (&extra_key, extra_key_seen),
        (&bad_reason, bad_reason_seen),
        (&reasoned, reasoned_seen),
    ] {
        pend(&scenario, id, "/notes/g.md", &json!({"text": "g"}));
        see(&scenario, id, &record);
    }
    let (columns, rows) = query(&scenario, pending_sql());
    let names = [
        "id", "board", "plugin", "op", "path", "replaces", "head", "bytes", "nul", "lines",
    ];
    assert_eq!(columns, json!(names));
    let record = format!(r#"{{"$type":"lr/task/v1","title":"{long}"}}"#);
    assert_eq!(
        rows,
        json!([
            row(&no_effect, "/notes/g.md", 0, "g", 1, 0, 1),
            row(&extra_key, "/notes/g.md", 0, "g", 1, 0, 1),
            row(&bad_reason, "/notes/g.md", 0, "g", 1, 0, 1),
            row(&a, "/notes/a.md", 1, "one\ntwo\n", 8, 0, 2),
            row(&b, "/tasks/b", 0, &record[..2000], record.len(), 0, 1),
            row(&d, "/notes/d.md", 0, "d", 1, 0, 1),
            row(&empty, "/notes/e.md", 0, "", 0, 0, 0),
            row(&malformed, "/notes/f.md", 0, "f", 1, 0, 1),
        ])
    );
}

#[test]
fn the_effects_mods_one_effect_query_finds_that_effect_while_no_seen_record_answers_it() {
    let [a, c] = ["a", "c"].map(|digit| digit.repeat(64));
    let scenario = board();
    pend(&scenario, &a, "/notes/a.md", &json!({"text": "a\n"}));
    pend(&scenario, &c, "/notes/c.md", &json!({"text": "c"}));
    see(&scenario, &c, &seen(&c, "refuse"));
    let found = json!([row(&a, "/notes/a.md", 0, "a\n", 2, 0, 1)]);
    assert_eq!(query(&scenario, &one_sql(&a)).1, found);
    assert_eq!(query(&scenario, &one_sql(&c)).1, json!([]));
}

#[test]
fn the_effects_mod_query_says_when_content_holds_a_nul_before_which_its_head_stops() {
    let id = "f".repeat(64);
    let scenario = board();
    let text = "harmless config\n\u{0}\nrm -rf /\nmore\n";
    pend(&scenario, &id, "/notes/f.md", &json!({"text": text}));
    // SQLite's text functions stop at the NUL, so `head` and `lines` cover
    // only "harmless config\n" while `bytes` counts all 32 bytes.
    let found = json!([row(&id, "/notes/f.md", 0, "harmless config\n", 32, 1, 1)]);
    assert_eq!(query(&scenario, pending_sql()).1, found);
}
