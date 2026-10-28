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

/// Writes a pending effect at `id` on `path`.
fn pend(scenario: &Scenario, id: &str, path: &str) {
    let effect = json!({"$type": "board/effect/v1", "board": "/b", "plugin": "notes",
        "op": "notes.publish", "path": path});
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
fn row(id: &str, path: &str) -> Value {
    json!([id, "/b", "notes", "notes.publish", path])
}

#[test]
fn the_effects_mod_lists_each_pending_effect_no_seen_record_answers() {
    let [a, b, c, d, malformed] = ["a", "b", "c", "d", "f"].map(|digit| digit.repeat(64));
    let [no_effect, extra_key, bad_reason, reasoned, null_reason] =
        ["1", "2", "3", "4", "5"].map(|digit| digit.repeat(64));
    let scenario = board();
    pend(&scenario, &a, "/notes/a.md");
    pend(&scenario, &b, "/tasks/b");
    pend(&scenario, &c, "/notes/c.md");
    see(&scenario, &c, &seen(&c, "approve"));
    // A record of another type where a seen record would be answers nothing.
    pend(&scenario, &d, "/notes/d.md");
    see(&scenario, &d, &json!({"$type": "lr/project/v1"}));
    // Nor does a seen record whose answer Board does not read.
    pend(&scenario, &malformed, "/notes/f.md");
    see(&scenario, &malformed, &seen(&malformed, "maybe"));
    let mut no_effect_seen = seen(&no_effect, "approve");
    no_effect_seen.as_object_mut().unwrap().remove("effect");
    let mut extra_key_seen = seen(&extra_key, "approve");
    extra_key_seen["note"] = json!("x");
    let mut bad_reason_seen = seen(&bad_reason, "refuse");
    bad_reason_seen["reason"] = json!(7);
    // A string reason is an answer, so that effect is not listed, and so is
    // a null one, which Board reads as no reason.
    let mut reasoned_seen = seen(&reasoned, "refuse");
    reasoned_seen["reason"] = json!("not now");
    let mut null_reason_seen = seen(&null_reason, "refuse");
    null_reason_seen["reason"] = Value::Null;
    for (id, record) in [
        (&no_effect, no_effect_seen),
        (&extra_key, extra_key_seen),
        (&bad_reason, bad_reason_seen),
        (&reasoned, reasoned_seen),
        (&null_reason, null_reason_seen),
    ] {
        pend(&scenario, id, "/notes/g.md");
        see(&scenario, id, &record);
    }
    let (columns, rows) = query(&scenario, pending_sql());
    assert_eq!(columns, json!(["id", "board", "plugin", "op", "path"]));
    assert_eq!(
        rows,
        json!([
            row(&no_effect, "/notes/g.md"),
            row(&extra_key, "/notes/g.md"),
            row(&bad_reason, "/notes/g.md"),
            row(&a, "/notes/a.md"),
            row(&b, "/tasks/b"),
            row(&d, "/notes/d.md"),
            row(&malformed, "/notes/f.md"),
        ])
    );
}

#[test]
fn the_effects_mods_one_effect_query_finds_that_effect_while_no_seen_record_answers_it() {
    let [a, c] = ["a", "c"].map(|digit| digit.repeat(64));
    let scenario = board();
    pend(&scenario, &a, "/notes/a.md");
    pend(&scenario, &c, "/notes/c.md");
    see(&scenario, &c, &seen(&c, "refuse"));
    let found = json!([row(&a, "/notes/a.md")]);
    assert_eq!(query(&scenario, &one_sql(&a)).1, found);
    assert_eq!(query(&scenario, &one_sql(&c)).1, json!([]));
}
