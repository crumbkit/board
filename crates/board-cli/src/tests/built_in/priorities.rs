//! The priorities plugin through the binary, in a board that commits each
//! put: what `set`, `move`, `drop` and `confirm` write, what `list` derives
//! from it, and each refusal beside an admitted twin, with no commit.

use super::*;

/// A board that enables tasks, changes' types, params and priorities,
/// holds two tasks and commits each put.
fn board() -> Fixture {
    let board = Fixture::enabling(&["tasks", "params", "priorities"]);
    board
        .write("tasks/release.md", &task("Release Board", "open"))
        .write("tasks/flare.md", &task("Rename Flare", "done"));
    board.committing();
    board
}

/// A `tasks/task/v2` task titled `title` with `status`.
fn task(title: &str, status: &str) -> String {
    format!("---\n$type: tasks/task/v2\ntitle: {title}\nstatus: {status}\n---\n# {title}\n")
}

impl Fixture {
    /// `priorities.set` with `input` as JSON on stdin, which must answer.
    fn set(&self, input: &Value) -> Value {
        let output = self.run_with(&["--json", "call", "priorities.set"], &input.to_string());
        assert_eq!(output.code, 0, "{}", text(&output.stdout));
        json(&output)["data"]["output"].clone()
    }

    /// The failure `priorities.set` prints for `input` on stdin.
    fn set_refused(&self, input: &Value) -> Value {
        let output = self.run_with(&["--json", "call", "priorities.set"], &input.to_string());
        assert_ne!(output.code, 0, "{}", text(&output.stdout));
        json(&output)
    }

    /// Each horizon's ids, in rank order.
    fn ranked(&self) -> (Vec<String>, Vec<String>) {
        let listed = self.call("priorities.list", &[]);
        let ids = |horizon: &str| {
            (listed[horizon].as_array().unwrap().iter())
                .map(|row| row["id"].as_str().unwrap().to_owned())
                .collect::<Vec<_>>()
        };
        (ids("short"), ids("long"))
    }
}

fn entry(horizon: &str, id: &str) -> Value {
    json!({"horizon": horizon, "id": id, "title": format!("Work on {id}")})
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

/// The UTC day today, as `confirm` records it.
fn today() -> String {
    jiff::Timestamp::now()
        .to_zoned(jiff::tz::TimeZone::UTC)
        .date()
        .to_string()
}

#[test]
fn list_with_no_record_gives_empty_horizons() {
    let board = board();
    assert_eq!(
        board.call("priorities.list", &[]),
        json!({"short": [], "long": [], "stale_after": 300.0})
    );
}

#[test]
fn set_writes_a_record_board_check_passes_and_list_ranks_each_horizon_with_what_it_covers() {
    let board = board();
    let mut release = entry("short", "release");
    release["covers"] = json!(["/tasks/release.md", "/tasks/flare.md"]);
    let output = board.set(&release);
    assert_eq!(
        output,
        json!({"id": "release", "horizon": "short", "rank": 1, "unconfirmed": true})
    );
    board.set(&entry("short", "flare"));
    board.set(&entry("long", "mxv"));
    assert!(
        board
            .board
            .head()
            .unwrap()
            .files
            .contains_key("priorities.json")
    );
    let listed = board.call("priorities.list", &[]);
    assert_eq!(
        listed["short"][0],
        json!({
            "rank": 1,
            "horizon": "short",
            "id": "release",
            "title": "Work on release",
            "confirmed": null,
            "unconfirmed": true,
            "stale": false,
            "covers": [
                {"path": "/tasks/release.md", "type": "tasks/task/v2", "title": "Release Board", "status": "open"},
                {"path": "/tasks/flare.md", "type": "tasks/task/v2", "title": "Rename Flare", "status": "done"},
            ],
        })
    );
    assert_eq!(
        (&listed["short"][1]["rank"], &listed["long"][0]["rank"]),
        (&json!(2), &json!(1))
    );
    assert_eq!(
        board.call("priorities.list", &["--horizon", "long"])["short"],
        json!([])
    );
    assert_eq!(
        board.checked("/priorities"),
        codes(&[("/priorities", None)])
    );
    board.delete("tasks/flare.md");
    let listed = board.call("priorities.list", &[]);
    assert_eq!(
        listed["short"][0]["covers"][1],
        json!({"path": "/tasks/flare.md", "missing": true})
    );
}

#[test]
fn set_at_move_drop_and_set_across_horizons_reorder_as_asked() {
    let board = board();
    for id in ["a", "b", "c"] {
        board.set(&entry("short", id));
    }
    let mut first = entry("short", "d");
    first["at"] = json!(1);
    board.set(&first);
    assert_eq!(board.ranked().0, ids(&["d", "a", "b", "c"]));
    let moved = board.call("priorities.move", &["--id", "c", "--to", "2"]);
    assert_eq!(moved["rank"], 2);
    assert_eq!(board.ranked().0, ids(&["d", "c", "a", "b"]));
    board.call("priorities.move", &["--id", "d", "--to", "99"]);
    assert_eq!(board.ranked().0, ids(&["c", "a", "b", "d"]));
    // A replaced entry keeps its place, and one set in the other horizon
    // moves there.
    board.set(&json!({"horizon": "short", "id": "a", "title": "A new title"}));
    board.set(&entry("long", "b"));
    assert_eq!(board.ranked(), (ids(&["c", "a", "d"]), ids(&["b"])));
    let dropped = board.call("priorities.drop", &["--id", "a"]);
    assert_eq!(
        dropped,
        json!({"id": "a", "horizon": "short", "rank": 2, "unconfirmed": true})
    );
    assert_eq!(board.ranked(), (ids(&["c", "d"]), ids(&["b"])));
}

#[test]
fn confirm_records_today_a_new_title_or_covers_clears_it_and_move_keeps_it() {
    let board = board();
    board.set(&entry("short", "a"));
    board.set(&entry("short", "b"));
    let confirmed = board.call("priorities.confirm", &["--id", "a"]);
    assert_eq!(confirmed, json!({"confirmed": ["a"], "day": today()}));
    board.call("priorities.move", &["--id", "a", "--to", "2"]);
    board.set(&entry("short", "a"));
    let row = |id: &str| {
        let listed = board.call("priorities.list", &[]);
        (listed["short"].as_array().unwrap().iter())
            .find(|row| row["id"] == id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (&row("a")["confirmed"], &row("a")["unconfirmed"]),
        (&json!(today()), &json!(false))
    );
    board.set(&json!({"horizon": "short", "id": "a", "title": "Another title"}));
    assert_eq!(row("a")["unconfirmed"], true);
    board.call("priorities.confirm", &["--all"]);
    let mut covered = entry("short", "b");
    covered["covers"] = json!(["/tasks/release.md"]);
    board.set(&covered);
    assert_eq!(
        (&row("a")["unconfirmed"], &row("b")["unconfirmed"]),
        (&json!(false), &json!(true))
    );
}

#[test]
fn an_entry_is_stale_once_the_board_has_more_commits_since_its_confirmation_than_the_value() {
    let board = board();
    board.set(&entry("short", "a"));
    board.set(&entry("short", "b"));
    board.call("priorities.confirm", &["--id", "a"]);
    board.call("priorities.move", &["--id", "a", "--to", "2"]);
    let stale = || {
        let listed = board.call("priorities.list", &[]);
        let flags: Vec<(Value, Value)> = (listed["short"].as_array().unwrap().iter())
            .map(|row| (row["id"].clone(), row["stale"].clone()))
            .collect();
        (listed["stale_after"].clone(), flags)
    };
    assert_eq!(
        stale(),
        (
            json!(300.0),
            vec![(json!("b"), json!(false)), (json!("a"), json!(false))]
        )
    );
    let heuristic = json!({
        "$type": "params/heuristic/v1",
        "summary": "A test's reconfirm value.",
        "unit": "commits",
        "inputs": [{"name": "commits", "meaning": "Commits.", "source": "Run git."}],
        "method": "Use one.",
        "start": 1,
        "default": 1,
        "bounds": {"min": 1, "max": 10},
        "enough": "Always.",
        "check": {"measure": "None.", "fails_above": 1},
        "recheck_after": 30,
        "consumer": "priorities",
    });
    board.write(
        "params/heuristics/priorities-reconfirm.json",
        &heuristic.to_string(),
    );
    // Only the confirmed entry can be stale.
    assert_eq!(
        stale(),
        (
            json!(1.0),
            vec![(json!("b"), json!(false)), (json!("a"), json!(true))]
        )
    );
}

/// Each refusal writes nothing and makes no commit; its twin, one value
/// apart, is admitted.
#[test]
fn each_refusal_writes_nothing_and_its_twin_is_admitted() {
    let board = board();
    for id in ["a", "b", "c", "d", "e", "f", "g"] {
        board.set(&entry("short", id));
    }
    let library = Scenario::new();
    let commit = library.commit(&[("tasks/x.md", task("Mounted", "open").as_str())]);
    library.track(commit);
    mount(&board, "repos/library.json", &library, commit);
    board.committing();
    let head = board.head();
    let covering = |covers: Value| {
        let mut input = entry("long", "h");
        input["covers"] = covers;
        input
    };
    let many: Vec<&str> = vec!["/tasks/release.md"; 21];
    let refusals = [
        (entry("short", "Not An Id"), "priorities::id::invalid"),
        (entry("short", "h"), "priorities::horizon::full"),
        (covering(json!(many)), "priorities::covers::too_many"),
        (
            covering(json!(["/tasks/nothing.md"])),
            "priorities::covers::wrong_type",
        ),
        (
            covering(json!(["/repos/library"])),
            "priorities::covers::wrong_type",
        ),
        (
            covering(json!(["/repos/library/tasks/x.md"])),
            "priorities::covers::through_mount",
        ),
    ];
    for (input, code) in refusals {
        let refused = board.set_refused(&input);
        assert_eq!(refused["code"], code, "{refused}");
        assert_eq!(board.head(), head, "{code}");
    }
    let full = board.set_refused(&entry("short", "h"));
    assert_eq!(full["fields"]["lowest"], "g");
    let unknown = board.refusal("priorities.move", &["--id", "z", "--to", "1"]);
    assert_eq!(unknown["code"], "priorities::id::not_found");
    let dropped = board.refusal("priorities.drop", &["--id", "z"]);
    assert_eq!(dropped["code"], "priorities::id::not_found");
    let unnamed = board.refusal("priorities.confirm", &[]);
    assert_eq!(unnamed["code"], "priorities::confirm::unnamed");
    let kept = board.read("priorities.json");
    board.write(
        "priorities.json",
        r#"{"$type": "priorities/list/v1", "short": []}"#,
    );
    let unreadable = board.refusal("priorities.drop", &["--id", "a"]);
    assert_eq!(unreadable["code"], "priorities::record::unreadable");
    board.write("priorities.json", &kept);
    assert_eq!(board.head(), head);
    // The twins.
    board.set(&entry("short", "a"));
    let mut twenty = covering(json!(vec!["/tasks/release.md"; 20]));
    board.set(&twenty);
    twenty["covers"] = json!(["/tasks/release.md"]);
    board.set(&twenty);
    board.call("priorities.move", &["--id", "h", "--to", "1"]);
    board.call("priorities.confirm", &["--id", "h"]);
    board.call("priorities.drop", &["--id", "g"]);
    board.set(&entry("short", "h"));
    assert_eq!(board.ranked().0, ids(&["a", "b", "c", "d", "e", "f", "h"]));
}
