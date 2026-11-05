//! Topics: each name becomes one topic by its slug, names the map sends to
//! a topic become its aliases, a map topic keeps its links, an existing
//! topic is left alone, and a dry run or a second run creates none.

use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::super::rules::{Context, MapError};
use super::super::{Options, run};
use super::{named, slug};

#[expect(
    clippy::disallowed_methods,
    reason = "a test writes the board it migrates"
)]
fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn read(root: &Path, relative: &str) -> Value {
    serde_json::from_str(&fs::read_to_string(root.join(relative)).unwrap()).unwrap()
}

#[test]
fn a_slug_keeps_lowercase_letters_and_digits_and_joins_the_rest_with_one_hyphen() {
    for (name, expected) in [
        ("Raycast Workspaces", Some("raycast-workspaces")),
        ("codex sandbox", Some("codex-sandbox")),
        ("codex-sandbox", Some("codex-sandbox")),
        ("claude-opus-5-5", Some("claude-opus-5-5")),
        ("  op inject!", Some("op-inject")),
        ("---", None),
    ] {
        assert_eq!(slug(name).as_deref(), expected, "{name}");
    }
    assert_eq!(named("/topics/seatbelt"), Some("seatbelt"));
    assert_eq!(named("/topics/Seat belt"), None);
    assert_eq!(named("/projects/seatbelt"), None);
}

/// A board whose evals and suites name tools by bare names, and a map that
/// sends `sandbox-exec` to the seatbelt topic and gives a topic for a name
/// two repositories hold.
fn board() -> (tempfile::TempDir, std::path::PathBuf) {
    let board = tempfile::tempdir().unwrap();
    let eval =
        |subjects: Value| json!({"$type": "evals/eval/v1", "subjects": subjects}).to_string();
    write(
        board.path(),
        "evals/a.json",
        &eval(json!(["seatbelt", "codex sandbox"])),
    );
    write(
        board.path(),
        "evals/b.json",
        &eval(json!(["sandbox-exec", "codex-sandbox"])),
    );
    write(
        board.path(),
        "evals/c.json",
        &eval(json!(["typescript-standards"])),
    );
    write(board.path(), "evals/d.json", &eval(json!(["uv"])));
    write(
        board.path(),
        "topics/uv.json",
        &json!({"$type": "topics/topic/v1", "title": "uv", "repos": ["astral-sh/uv"]}).to_string(),
    );
    let map = board.path().join(".map.json");
    let standards = json!({
        "$type": "topics/topic/v1",
        "title": "typescript-standards",
        "repos": ["crumbkit/typescript-standards", "cogentforge/typescript-standards"],
    });
    let text = json!({"sandbox-exec": "/topics/seatbelt", "typescript-standards": standards});
    write(board.path(), ".map.json", &text.to_string());
    (board, map)
}

#[test]
fn each_name_becomes_one_topic_and_the_names_the_map_or_its_slug_merge_become_its_aliases() {
    let (board, map) = board();
    let root = board.path();
    let options = Options {
        write: true,
        map: Some(map),
    };
    let report = run(root, &options).unwrap();
    assert_eq!(report["unmapped"], json!([]));
    let created: Vec<&str> = (report["topics"].as_array().unwrap().iter())
        .map(|topic| topic["path"].as_str().unwrap())
        .collect();
    assert_eq!(
        created,
        [
            "/topics/codex-sandbox",
            "/topics/seatbelt",
            "/topics/typescript-standards"
        ]
    );
    assert_eq!(
        read(root, "evals/a.json")["subjects"],
        json!(["/topics/seatbelt", "/topics/codex-sandbox"])
    );
    assert_eq!(
        read(root, "evals/b.json")["subjects"],
        json!(["/topics/seatbelt", "/topics/codex-sandbox"])
    );
    assert_eq!(
        read(root, "topics/seatbelt.json"),
        json!({"$type": "topics/topic/v1", "title": "seatbelt", "aliases": ["sandbox-exec"]})
    );
    assert_eq!(
        read(root, "topics/codex-sandbox.json"),
        json!({"$type": "topics/topic/v1", "title": "codex-sandbox", "aliases": ["codex sandbox"]})
    );
    assert_eq!(
        read(root, "topics/typescript-standards.json")["repos"],
        json!([
            "crumbkit/typescript-standards",
            "cogentforge/typescript-standards"
        ])
    );
    // A topic already in the board is named and left as it was.
    assert_eq!(
        read(root, "evals/d.json")["subjects"],
        json!(["/topics/uv"])
    );
    assert_eq!(
        read(root, "topics/uv.json")["repos"],
        json!(["astral-sh/uv"])
    );
    let again = run(root, &options).unwrap();
    assert_eq!(
        (&again["files"]["changed"], &again["topics"]),
        (&json!(0), &json!([]))
    );
}

#[test]
fn a_dry_run_lists_the_topics_it_would_create_and_writes_none() {
    let (board, map) = board();
    let report = run(
        board.path(),
        &Options {
            write: false,
            map: Some(map),
        },
    )
    .unwrap();
    assert_eq!(report["topics"].as_array().unwrap().len(), 3);
    assert!(!board.path().join("topics/seatbelt.json").exists());
    assert_eq!(report["files"]["written"], json!(0));
}

/// A map topic must pass the topic schema and have a title with a slug.
#[test]
fn a_map_topic_without_a_usable_title_or_with_a_bare_repo_is_refused_and_its_twin_is_admitted() {
    let topic = |fields: Value| {
        let mut topic = json!({"$type": "topics/topic/v1", "title": "typescript-standards"});
        topic
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        json!({ "ts": topic })
    };
    for refused in [
        topic(json!({"title": "---"})),
        topic(json!({"repos": ["typescript-standards"]})),
        topic(json!({"colour": "blue"})),
    ] {
        assert_eq!(
            Context::new(refused).err(),
            Some(MapError::Entry("ts".into()))
        );
    }
    assert!(Context::new(topic(json!({"repos": ["crumbkit/typescript-standards"]}))).is_ok());
}

/// A map key that gives a topic for a subject names no actor, so the same
/// old text as an actor is read as text, never as the topic record.
#[test]
fn a_map_topic_key_that_is_also_an_actors_text_leaves_the_actor_read_from_the_text() {
    let board = tempfile::tempdir().unwrap();
    let root = board.path();
    let topic = json!({"$type": "topics/topic/v1", "title": "Widget Kit"});
    write(root, ".map.json", &json!({"Widget Kit": topic}).to_string());
    let eval = json!({"$type": "evals/eval/v1", "subjects": ["Widget Kit"]});
    write(root, "evals/a.json", &eval.to_string());
    let run_record = json!({"$type": "runs/run/v1", "by": "Widget Kit"});
    write(root, "runs/a.json", &run_record.to_string());
    let options = Options {
        write: true,
        map: Some(root.join(".map.json")),
    };
    run(root, &options).unwrap();
    assert_eq!(
        read(root, "evals/a.json")["subjects"],
        json!(["/topics/widget-kit"])
    );
    assert_eq!(
        read(root, "runs/a.json")["by"],
        json!({"agent": "Widget Kit"})
    );
}

/// A topic the board already holds, as JSON or Markdown, is not written
/// again, and the run reports the names it would have added to it.
#[test]
fn a_topic_the_board_holds_as_json_or_markdown_is_kept_and_the_names_it_would_gain_are_reported() {
    let board = tempfile::tempdir().unwrap();
    let root = board.path();
    write(
        root,
        "topics/uv-tool.md",
        "---\n$type: topics/topic/v1\ntitle: uv tool\n---\n",
    );
    write(
        root,
        "evals/a.json",
        &json!({"$type": "evals/eval/v1", "subjects": ["uv tool", "uv-tool"]}).to_string(),
    );
    let report = run(
        root,
        &Options {
            write: true,
            map: None,
        },
    )
    .unwrap();
    assert!(!root.join("topics/uv-tool.json").exists());
    assert_eq!(report["topics"], json!([]));
    assert_eq!(
        report["topics_existing"],
        json!([{"path": "/topics/uv-tool", "names": ["uv tool", "uv-tool"]}])
    );
}
