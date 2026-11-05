use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::rules::{self, Context, MapError, Outcome};
use super::{Options, run};

fn context() -> Context {
    Context::new(json!({"Board app": "crumbkit/board-app", "odd one": {"person": "luke"}})).unwrap()
}

/// The outcome of a subject `name` that now names the topic `slug`.
fn topic(slug: &str, name: &str) -> Outcome {
    Outcome::Topic {
        slug: slug.into(),
        name: name.into(),
        given: None,
    }
}

fn migrated(record: Value) -> (Value, Vec<Outcome>) {
    let mut record = record;
    let outcomes = rules::record(&mut record, "r", &context(), &|_| None);
    (record, outcomes)
}

#[test]
fn old_actor_text_becomes_an_actor_with_the_parts_it_names() {
    let cases = [
        (
            "local_84ccdcef-4c52",
            json!({"session": "local_84ccdcef-4c52"}),
        ),
        (
            "Codex recording builder",
            json!({"program": "codex", "agent": "Codex recording builder"}),
        ),
        (
            "claude-opus-5-5 subagent",
            json!({"program": "claude-code", "agent": "claude-opus-5-5 subagent"}),
        ),
        (
            "clean-context sub-agent",
            json!({"program": "claude-code", "agent": "clean-context sub-agent"}),
        ),
        ("Codex", json!({"program": "codex"})),
        ("claude", json!({"program": "claude-code"})),
        (
            "Luke, relayed by the session local_7455.",
            json!({
                "session": "local_7455",
                "person": "luke",
                "agent": "Luke, relayed by the session local_7455."
            }),
        ),
        ("Luke", json!({"person": "luke"})),
        (
            "Codex in local_7455",
            json!({"session": "local_7455", "program": "codex", "agent": "Codex in local_7455"}),
        ),
        ("odd one", json!({"person": "luke"})),
    ];
    for (text, actor) in cases {
        assert_eq!(rules::actor(text, &context()), Some(actor), "{text}");
    }
}

/// Text that names no session, program or person is the agent's name, and
/// only blank text names nobody.
#[test]
fn actor_text_that_names_no_part_is_the_agent_and_blank_text_names_nobody() {
    assert_eq!(
        rules::actor("c2-completion-codec-builder", &context()),
        Some(json!({"agent": "c2-completion-codec-builder"}))
    );
    assert_eq!(
        rules::actor("c2-completion-codex-builder", &context()),
        Some(json!({"program": "codex", "agent": "c2-completion-codex-builder"}))
    );
    assert_eq!(rules::actor(" ", &context()), None);
}

#[test]
fn a_question_session_becomes_its_by() {
    let (record, outcomes) = migrated(json!({
        "$type": "plans/question/v1", "session": "local_ab12", "question": "q"
    }));
    assert_eq!(
        record,
        json!({"$type": "plans/question/v1", "by": {"session": "local_ab12"}, "question": "q"})
    );
    assert_eq!(outcomes, [Outcome::Changed("plans/question/v1.session")]);
}

/// A question's `by` must hold a session, so a question whose session is a
/// name is left and reported, while a decision records the name as its
/// agent.
#[test]
fn a_question_whose_session_is_a_name_is_reported_and_a_decision_records_it_as_its_agent() {
    let before = json!({"$type": "plans/question/v1", "session": "xenodochial-knuth"});
    let (record, outcomes) = migrated(before.clone());
    assert_eq!(record, before);
    assert_eq!(
        outcomes,
        [Outcome::Unmapped(
            "plans/question/v1.session",
            "xenodochial-knuth".into()
        )]
    );
    let (record, outcomes) =
        migrated(json!({"$type": "plans/decision/v1", "session": "xenodochial-knuth"}));
    assert_eq!(
        record,
        json!({"$type": "plans/decision/v1", "by": {"agent": "xenodochial-knuth"}})
    );
    assert_eq!(outcomes, [Outcome::Changed("plans/decision/v1.session")]);
}

/// An optional `by` keeps a name as its agent, and only a blank one goes.
#[test]
fn an_optional_by_that_is_blank_is_dropped_and_its_twin_keeps_the_name_as_its_agent() {
    let (record, outcomes) = migrated(json!({"$type": "runs/run/v1", "by": " "}));
    assert_eq!(record, json!({"$type": "runs/run/v1"}));
    assert_eq!(outcomes, [Outcome::Dropped("runs/run/v1.by", " ".into())]);
    let (record, outcomes) = migrated(json!({"$type": "runs/run/v1", "by": "ui-lead-01a1"}));
    assert_eq!(
        record,
        json!({"$type": "runs/run/v1", "by": {"agent": "ui-lead-01a1"}})
    );
    assert_eq!(outcomes, [Outcome::Changed("runs/run/v1.by")]);
}

#[test]
fn an_actor_already_in_the_new_shape_is_left_alone() {
    for record in [
        json!({"$type": "runs/run/v1", "by": {"program": "codex"}}),
        json!({"$type": "plans/question/v1", "by": {"session": "local_ab"}}),
        json!({"$type": "artifacts/artifact/v1", "account": {"person": "a@b.c"}}),
        json!({"$type": "board/seen/v1", "effect": format!("/effects/answered/{}", "ab".repeat(32))}),
        json!({"$type": "board/seen/v1", "effect": format!("/effects/pending/{}", "ab".repeat(32))}),
        json!({"$type": "requests/reply/v1", "effect": format!("sha256:{}", "ab".repeat(32))}),
        json!({"$type": "evals/eval/v1", "subjects": ["crumbkit/board", "/repos/board"]}),
    ] {
        assert_eq!(migrated(record.clone()), (record, vec![]));
    }
}

#[test]
fn an_artifact_account_becomes_a_person_and_a_version_session_its_by() {
    let (account, _) = migrated(json!({"$type": "artifacts/artifact/v1", "account": "a@b.c"}));
    assert_eq!(account["account"], json!({"person": "a@b.c"}));
    let (version, _) = migrated(json!({"$type": "artifacts/version/v1", "session": "local_ab"}));
    assert_eq!(
        version,
        json!({"$type": "artifacts/version/v1", "by": {"session": "local_ab"}})
    );
}

#[test]
fn bare_hashes_gain_their_prefix_and_a_seen_effect_keeps_the_path_board_app_reads() {
    let hex = "ab".repeat(32);
    let prefixed = json!(format!("sha256:{hex}"));
    for (kind, key) in [
        ("requests/reply/v1", "effect"),
        ("boundary/crossing/v1", "digest"),
        ("boundary/crossing/v1", "override"),
        ("artifacts/version/v1", "sha256"),
    ] {
        let (record, _) = migrated(json!({"$type": kind, key: hex}));
        assert_eq!(record[key], prefixed, "{kind}.{key}");
    }
    let effect = json!({"$type": "board/effect/v1", "digest": format!("sha256:{hex}")});
    assert_eq!(migrated(effect.clone()), (effect, vec![]));
}

#[test]
fn subjects_keep_valid_ones_and_map_or_resolve_the_rest() {
    let context = context();
    let mounts = |name: &str| (name == "board").then(|| "crumbkit/board".to_owned());
    let mut record = json!({"$type": "runs/suite/v1", "subjects": [
        "octo/core", "board", "Board app", "Raycast Workspaces", "/repos/x", "crumbkit/board"
    ]});
    let outcomes = rules::record(&mut record, "s", &context, &mounts);
    assert_eq!(
        record["subjects"],
        json!([
            "octo/core",
            "crumbkit/board",
            "crumbkit/board-app",
            "/topics/raycast-workspaces",
            "/repos/x"
        ])
    );
    assert_eq!(
        outcomes,
        [
            Outcome::Changed("runs/suite/v1.subjects"),
            Outcome::Changed("runs/suite/v1.subjects"),
            Outcome::Changed("runs/suite/v1.subjects"),
            topic("raycast-workspaces", "Raycast Workspaces"),
        ]
    );
}

/// A suite or eval is not its own subject, so its own name goes. A suite
/// left with no subject has none, while an eval must keep one, so an eval
/// whose only subject is its own name keeps it and it is reported.
#[test]
fn a_record_loses_its_own_name_as_a_subject_unless_it_is_an_evals_only_one_which_becomes_a_topic() {
    for kind in ["runs/suite/v1", "evals/eval/v1"] {
        let mut more = json!({"$type": kind, "subjects": ["pipecat-ci", "octo/pipecat"]});
        rules::record(&mut more, "pipecat-ci", &context(), &|_| None);
        assert_eq!(more["subjects"], json!(["octo/pipecat"]), "{kind}");
        let mut other = json!({"$type": kind, "subjects": ["pipecat-ci"]});
        let outcomes = rules::record(&mut other, "other", &context(), &|_| None);
        assert_eq!(other["subjects"], json!(["/topics/pipecat-ci"]));
        assert!(matches!(outcomes[..], [_, Outcome::Topic { .. }]), "{kind}");
    }
    let mut suite = json!({"$type": "runs/suite/v1", "subjects": ["pipecat-ci"]});
    let outcomes = rules::record(&mut suite, "pipecat-ci", &context(), &|_| None);
    assert_eq!(
        (suite, outcomes),
        (
            json!({"$type": "runs/suite/v1"}),
            vec![Outcome::Changed("runs/suite/v1.subjects")]
        )
    );
    let mut eval = json!({"$type": "evals/eval/v1", "subjects": ["pipecat-ci"]});
    let outcomes = rules::record(&mut eval, "pipecat-ci", &context(), &|_| None);
    assert_eq!(eval["subjects"], json!(["/topics/pipecat-ci"]));
    assert_eq!(
        outcomes,
        [
            Outcome::Changed("evals/eval/v1.subjects"),
            topic("pipecat-ci", "pipecat-ci")
        ]
    );
}

/// A map entry that is neither an `Actor` nor a `Subject` is refused by
/// its key, and its twin is admitted.
#[test]
fn a_map_entry_outside_the_shapes_is_refused_by_its_key_and_its_twin_is_admitted() {
    for (refused, admitted) in [
        (json!({"program": "Codex"}), json!({"program": "codex"})),
        (json!({"model": "opus"}), json!({"agent": "opus reviewer"})),
        (json!("pipecat"), json!("octo/pipecat")),
    ] {
        let error = Context::new(json!({"old": refused})).err();
        assert_eq!(error, Some(MapError::Entry("old".into())), "{refused}");
        assert!(Context::new(json!({"old": admitted})).is_ok(), "{admitted}");
    }
}

/// A map file that is not a JSON object, or whose entry with an empty key
/// maps to neither shape, is refused with its own code before anything is
/// written, and an object map is admitted.
#[test]
fn a_map_file_that_is_not_an_object_is_refused_and_its_twin_is_admitted() {
    let board = tempfile::tempdir().unwrap();
    write(board.path(), "maps/array.json", "[]");
    write(board.path(), "maps/object.json", "{}");
    let options = |name: &str| Options {
        map: Some(board.path().join(format!("maps/{name}.json"))),
        ..Options::default()
    };
    write(board.path(), "maps/blank.json", r#"{"": 5}"#);
    let refused = run(board.path(), &options("array")).unwrap_err();
    assert_eq!(refused.code(), "xtask::migrate::map_invalid");
    let refused = run(board.path(), &options("blank")).unwrap_err();
    assert_eq!(refused.code(), "xtask::migrate::map_entry_invalid");
    assert!(run(board.path(), &options("object")).is_ok());
}

#[test]
fn a_task_owner_becomes_an_actor_and_a_blank_one_is_kept_and_reported() {
    let before = json!({"$type": "tasks/task/v2", "owner": " "});
    let (record, outcomes) = migrated(before.clone());
    assert_eq!(record, before);
    assert_eq!(
        outcomes,
        [Outcome::Unmapped("tasks/task/v2.owner", " ".into())]
    );
    let (record, _) = migrated(json!({"$type": "tasks/task/v2", "owner": "lead-7f3a"}));
    assert_eq!(record["owner"], json!({"agent": "lead-7f3a"}));
}

#[test]
fn a_version_session_the_released_mod_wrote_becomes_the_mods_program_keeping_the_uuid() {
    let (record, _) =
        migrated(json!({"$type": "artifacts/version/v1", "session": "0b8a7c6e-1f2d"}));
    assert_eq!(
        record,
        json!({"$type": "artifacts/version/v1", "by": {"program": "claude-code", "agent": "0b8a7c6e-1f2d"}})
    );
}

#[test]
fn a_hash_that_is_neither_bare_hex_nor_a_digest_is_reported_and_its_twin_is_prefixed() {
    for effect in ["ab", "sha256:ab"] {
        let (record, outcomes) = migrated(json!({"$type": "requests/reply/v1", "effect": effect}));
        assert_eq!(record["effect"], json!(effect));
        assert_eq!(
            outcomes,
            [Outcome::Unmapped("requests/reply/v1.effect", effect.into())]
        );
    }
}

#[test]
fn a_subject_grammar_matches_the_shape() {
    for valid in [
        "crumbkit/board",
        "rust-lang/cargo",
        "o/n.rs",
        "/repos/board",
    ] {
        assert!(rules::subject(valid), "{valid}");
    }
    for invalid in ["board", "@kubb/plugin-zod", "a/b/c", "/", "/a b", "o/"] {
        assert!(!rules::subject(invalid), "{invalid}");
    }
}

#[test]
fn a_github_remote_names_its_repository() {
    for url in [
        "https://github.com/crumbkit/board.git",
        "https://github.com/crumbkit/board",
        "git@github.com:crumbkit/board.git",
    ] {
        assert_eq!(
            rules::github(url).as_deref(),
            Some("crumbkit/board"),
            "{url}"
        );
    }
    assert_eq!(rules::github("/Users/luke/Code/x"), None);
}

#[test]
fn a_task_owner_line_in_frontmatter_becomes_an_actor() {
    let text = "---\n$type: \"tasks/task/v2\"\nowner: Codex integrator\ntitle: t\n---\nbody\n";
    let (migrated, outcomes) = rules::markdown(text, &context()).unwrap();
    assert_eq!(
        migrated,
        "---\n$type: \"tasks/task/v2\"\nowner: {\"agent\":\"Codex integrator\",\"program\":\"codex\"}\ntitle: t\n---\nbody\n"
    );
    assert_eq!(outcomes, [Outcome::Changed("tasks/task/v2.owner")]);
    let text = "---\n$type: tasks/task/v2\nowner: \"c2_b_builder\"\n---\n";
    let (named, outcomes) = rules::markdown(text, &context()).unwrap();
    assert_eq!(
        named,
        "---\n$type: tasks/task/v2\nowner: {\"agent\":\"c2_b_builder\"}\n---\n"
    );
    assert_eq!(outcomes, [Outcome::Changed("tasks/task/v2.owner")]);
    let done = "---\n$type: tasks/task/v2\nowner: {\"person\":\"luke\"}\n---\n";
    assert_eq!(rules::markdown(done, &context()), None);
    assert_eq!(
        rules::markdown("---\n$type: prompts/prompt/v1\nowner: x\n---\n", &context()),
        None
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "a test writes the board it migrates"
)]
fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

#[test]
fn a_dry_run_counts_and_writes_nothing_and_a_write_is_idempotent() {
    let board = tempfile::tempdir().unwrap();
    let root = board.path();
    let run_record = "{\n  \"$type\": \"runs/run/v1\",\n  \"by\": \"claude\"\n}\n";
    write(root, "suites/s/runs/r.json", run_record);
    write(
        root,
        "tasks/t.md",
        "---\n$type: tasks/task/v2\nowner: codex\n---\n",
    );
    write(
        root,
        "notes/n.json",
        "{\"$type\": \"other/thing/v1\", \"by\": \"claude\"}",
    );
    write(
        root,
        ".git/x.json",
        "{\"$type\": \"runs/run/v1\", \"by\": \"claude\"}",
    );
    let dry = run(root, &Options::default()).unwrap();
    assert_eq!(dry["files"], json!({"read": 3, "changed": 2, "written": 0}));
    assert_eq!(
        dry["changed"],
        json!({"runs/run/v1.by": 1, "tasks/task/v2.owner": 1})
    );
    assert_eq!(
        fs::read_to_string(root.join("suites/s/runs/r.json")).unwrap(),
        run_record
    );

    let written = run(
        root,
        &Options {
            write: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(written["files"]["written"], json!(2));
    assert_eq!(
        fs::read_to_string(root.join("suites/s/runs/r.json")).unwrap(),
        "{\n  \"$type\": \"runs/run/v1\",\n  \"by\": {\n    \"program\": \"claude-code\"\n  }\n}\n"
    );
    let again = run(
        root,
        &Options {
            write: true,
            ..Options::default()
        },
    )
    .unwrap();
    assert_eq!(
        again["files"],
        json!({"read": 3, "changed": 0, "written": 0})
    );
}

/// A rewritten record keeps its keys in the order its file had them, and a
/// renamed field takes the old field's place.
#[test]
fn a_rewritten_record_keeps_its_key_order_and_a_renamed_field_takes_the_old_place() {
    let board = tempfile::tempdir().unwrap();
    let root = board.path();
    write(
        root,
        "plans/p/decisions/d.json",
        r#"{"title": "t", "session": "local_ab", "$type": "plans/decision/v1", "refs": [{"z": 1, "a": 2}]}"#,
    );
    let options = Options {
        write: true,
        ..Options::default()
    };
    run(root, &options).unwrap();
    assert_eq!(
        fs::read_to_string(root.join("plans/p/decisions/d.json")).unwrap(),
        "{\n  \"title\": \"t\",\n  \"by\": {\n    \"session\": \"local_ab\"\n  },\n  \
         \"$type\": \"plans/decision/v1\",\n  \"refs\": [\n    {\n      \"z\": 1,\n      \
         \"a\": 2\n    }\n  ]\n}\n"
    );
}

#[test]
fn a_subject_naming_a_mount_resolves_through_the_mount_chain() {
    let board = tempfile::tempdir().unwrap();
    let nested = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let git = |arguments: &[&str]| {
        assert!(
            crate::process::command("git")
                .arg("-C")
                .arg(repo.path())
                .args(arguments)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "-q"]);
    git(&[
        "remote",
        "add",
        "origin",
        "git@github.com:crumbkit/core.git",
    ]);
    let mount = |repo: &Path| json!({"$type": "board/mount/v1", "repo": repo}).to_string();
    write(board.path(), "repos/outer.json", &mount(nested.path()));
    write(
        board.path(),
        "repos/core.json",
        &json!({"$type": "board/mount/v1", "path": "/repos/outer/repos/core"}).to_string(),
    );
    write(nested.path(), "repos/core.json", &mount(repo.path()));
    write(
        board.path(),
        "evals/e.json",
        &json!({"$type": "evals/eval/v1", "subjects": ["core", "outer"]}).to_string(),
    );
    let report = run(
        board.path(),
        &Options {
            write: true,
            ..Options::default()
        },
    )
    .unwrap();
    let eval: Value =
        serde_json::from_str(&fs::read_to_string(board.path().join("evals/e.json")).unwrap())
            .unwrap();
    assert_eq!(eval["subjects"], json!(["crumbkit/core", "/topics/outer"]));
    assert_eq!(report["unmapped"], json!([]));
    assert_eq!(
        report["topics"],
        json!([{"path": "/topics/outer", "record": {"$type": "topics/topic/v1", "title": "outer"}}])
    );
}

/// Each committed type schema in the workspace's crates, compiled, by type.
fn schemas() -> std::collections::BTreeMap<String, board_validate::Schema> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../crates");
    let mut found = std::collections::BTreeMap::new();
    for entry in fs::read_dir(&root).unwrap() {
        let types = entry.unwrap().path().join("types");
        let mut pending = vec![types.clone()];
        while let Some(directory) = pending.pop() {
            let Ok(entries) = fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                let name = path
                    .strip_prefix(&types)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                if path.is_dir() {
                    pending.push(path);
                } else if let Some(kind) = name.strip_suffix(".schema.json") {
                    let schema = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
                    found.insert(
                        kind.to_owned(),
                        board_validate::Schema::compile(schema).unwrap(),
                    );
                }
            }
        }
    }
    found
}

/// The board paths of the JSON records under `board` that a committed type
/// schema refuses.
fn refused(
    board: &Path,
    schemas: &std::collections::BTreeMap<String, board_validate::Schema>,
) -> std::collections::BTreeSet<String> {
    let mut refused = std::collections::BTreeSet::new();
    let actor = board_plugin::shape::registry()["Actor"].clone();
    let actor = board_validate::Schema::compile(actor).unwrap();
    for path in super::files(board).unwrap() {
        let text = fs::read_to_string(&path).unwrap();
        let name = format!("/{}", path.strip_prefix(board).unwrap().display());
        if path.extension().is_some_and(|extension| extension == "md") {
            if owner(&text).is_some_and(|owner| actor.validate(&owner).is_err()) {
                refused.insert(name);
            }
            continue;
        }
        let Ok(record) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let kind = record
            .get("$type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if schemas
            .get(kind)
            .is_some_and(|schema| schema.validate(&record).is_err())
        {
            refused.insert(name);
        }
    }
    refused
}

/// The owner a `tasks/task/v2` Markdown record's frontmatter gives, as
/// JSON, or the text itself when it is not JSON; none when it has none.
fn owner(text: &str) -> Option<Value> {
    let front = text.strip_prefix("---\n")?.split("\n---").next()?;
    let task = front.lines().any(|line| {
        line.strip_prefix("$type:")
            .is_some_and(|kind| kind.trim().trim_matches('"') == "tasks/task/v2")
    });
    let line = front
        .lines()
        .find_map(|line| line.strip_prefix("owner:"))?
        .trim();
    (task && !line.is_empty() && line != "null")
        .then(|| serde_json::from_str(line).unwrap_or_else(|_| Value::String(line.to_owned())))
}

/// Migrates a copy of the board `MIGRATE_BOARD` names, when it is set, and
/// checks that every record it changed passes its type's committed schema,
/// that each one it reported unmapped passes once those values are
/// replaced, and that a second run changes nothing. A record the schemas
/// refuse that the run left byte for byte as it was is not the migration's.
/// It never writes the board itself.
#[test]
fn a_copy_of_a_real_board_migrates_and_a_second_run_changes_nothing() {
    let Some(source) = crate::process::var("MIGRATE_BOARD") else {
        return;
    };
    let copy = tempfile::tempdir().unwrap();
    let board = copy.path().join("board");
    assert!(
        crate::process::command("cp")
            .arg("-R")
            .arg(&source)
            .arg(&board)
            .status()
            .unwrap()
            .success()
    );
    let map = crate::process::var("MIGRATE_MAP").map(std::path::PathBuf::from);
    let options = Options { write: true, map };
    let schemas = schemas();
    let read = |path: &String| fs::read(board.join(&path[1..])).ok();
    let before: std::collections::BTreeMap<String, Option<Vec<u8>>> = (refused(&board, &schemas)
        .into_iter())
    .map(|path| {
        let bytes = read(&path);
        (path, bytes)
    })
    .collect();
    let first = run(&board, &options).unwrap();
    assert_ne!(first["files"]["read"], json!(0));
    let unmapped: std::collections::BTreeSet<String> = (first["unmapped"].as_array().unwrap())
        .iter()
        .filter_map(|entry| entry["path"].as_str().map(str::to_owned))
        .collect();
    let after = refused(&board, &schemas);
    let changed: Vec<&String> = (after.iter())
        .filter(|path| !unmapped.contains(*path))
        .filter(|path| before.get(*path) != Some(&read(path)))
        .collect();
    assert_eq!(changed, Vec::<&String>::new());
    // A record excused for an unmapped subject passes once that subject is
    // a valid one, so no other field it holds is refused.
    let values: Vec<(&str, &Value)> = (first["unmapped"].as_array().unwrap().iter())
        .map(|entry| (entry["path"].as_str().unwrap(), &entry["value"]))
        .collect();
    for path in after.intersection(&unmapped).filter(|path| {
        Path::new(path.as_str())
            .extension()
            .is_some_and(|extension| extension == "json")
    }) {
        let mut record: Value =
            serde_json::from_str(&fs::read_to_string(board.join(&path[1..])).unwrap()).unwrap();
        if let Some(Value::Array(subjects)) = record.get_mut("subjects") {
            for subject in subjects {
                if values.contains(&(path.as_str(), &*subject)) {
                    *subject = json!("/unmapped");
                }
            }
        }
        let kind = record["$type"].as_str().unwrap_or_default().to_owned();
        let refused = schemas[&kind].validate(&record).err();
        assert!(refused.is_none(), "{path}: {refused:?}");
    }
    let second = run(&board, &options).unwrap();
    assert_eq!(second["files"]["changed"], json!(0));
}

/// A mount's `repo` that starts with `~/` is read from the home directory,
/// and any other path is read as it is.
#[test]
fn a_home_relative_mount_path_is_read_from_the_home_directory_and_its_twin_as_it_is() {
    let home = super::home("~/Code/x");
    assert!(
        home.is_absolute() && home.ends_with("Code/x"),
        "{}",
        home.display()
    );
    assert_eq!(super::home("/srv/x"), std::path::PathBuf::from("/srv/x"));
}
