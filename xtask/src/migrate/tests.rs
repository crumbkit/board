use std::fs;
use std::path::Path;

use serde_json::{Value, json};

use super::rules::{self, Context, Outcome};
use super::{Options, run};

fn context() -> Context {
    Context::new(json!({"Board app": "crumbkit/board-app", "odd one": {"person": "luke"}}))
}

fn migrated(record: Value) -> (Value, Vec<Outcome>) {
    let mut record = record;
    let outcomes = rules::record(&mut record, &context(), &|_| None);
    (record, outcomes)
}

#[test]
fn old_actor_text_becomes_an_actor_with_the_parts_it_names() {
    let cases = [
        (
            "local_84ccdcef-4c52",
            json!({"session": "local_84ccdcef-4c52"}),
        ),
        ("Codex recording builder", json!({"program": "codex"})),
        (
            "claude-opus-5-5 subagent",
            json!({"program": "claude-code"}),
        ),
        ("clean-context sub-agent", json!({"program": "claude-code"})),
        (
            "Luke, relayed by the session local_7455.",
            json!({"session": "local_7455", "person": "luke"}),
        ),
        ("odd one", json!({"person": "luke"})),
    ];
    for (text, actor) in cases {
        assert_eq!(rules::actor(text, &context()), Some(actor), "{text}");
    }
}

#[test]
fn actor_text_that_names_nobody_maps_to_nothing_and_its_twin_does() {
    assert_eq!(
        rules::actor("c2-completion-codec-builder", &context()),
        None
    );
    assert_eq!(
        rules::actor("re-review of lukerandall/auth#4", &context()),
        None
    );
    assert_eq!(
        rules::actor("c2-completion-codex-builder", &context()),
        Some(json!({"program": "codex"}))
    );
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

#[test]
fn a_decision_whose_session_names_nobody_is_left_and_reported() {
    let before = json!({"$type": "plans/decision/v1", "session": "xenodochial-knuth"});
    let (record, outcomes) = migrated(before.clone());
    assert_eq!(record, before);
    assert_eq!(
        outcomes,
        [Outcome::Unmapped(
            "plans/decision/v1.session",
            "xenodochial-knuth".into()
        )]
    );
}

#[test]
fn an_optional_by_that_names_nobody_is_dropped_and_reported() {
    let (record, outcomes) = migrated(json!({"$type": "runs/run/v1", "by": "ui-lead-01a1"}));
    assert_eq!(record, json!({"$type": "runs/run/v1"}));
    assert_eq!(
        outcomes,
        [Outcome::Dropped("runs/run/v1.by", "ui-lead-01a1".into())]
    );
}

#[test]
fn an_actor_already_in_the_new_shape_is_left_alone() {
    for record in [
        json!({"$type": "runs/run/v1", "by": {"program": "codex"}}),
        json!({"$type": "plans/question/v1", "by": {"session": "local_ab"}}),
        json!({"$type": "artifacts/artifact/v1", "account": {"person": "a@b.c"}}),
        json!({"$type": "board/seen/v1", "effect": format!("sha256:{}", "ab".repeat(32))}),
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
fn bare_hashes_gain_their_prefix_and_a_seen_effect_path_becomes_its_digest() {
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
    for effect in [format!("/effects/answered/{hex}"), hex.clone()] {
        let (seen, outcomes) = migrated(json!({"$type": "board/seen/v1", "effect": effect}));
        assert_eq!(seen["effect"], prefixed);
        assert_eq!(outcomes, [Outcome::Changed("board/seen/v1.effect")]);
    }
    let effect = json!({"$type": "board/effect/v1", "digest": format!("sha256:{hex}")});
    assert_eq!(migrated(effect.clone()), (effect, vec![]));
}

#[test]
fn subjects_keep_valid_ones_and_map_or_resolve_the_rest() {
    let context = context();
    let mounts = |name: &str| (name == "board").then(|| "crumbkit/board".to_owned());
    let mut record = json!({"$type": "runs/suite/v1", "subjects": [
        "octo/core", "board", "Board app", "Raycast Workspaces", "/repos/x"
    ]});
    let outcomes = rules::record(&mut record, &context, &mounts);
    assert_eq!(
        record["subjects"],
        json!([
            "octo/core",
            "crumbkit/board",
            "crumbkit/board-app",
            "Raycast Workspaces",
            "/repos/x"
        ])
    );
    assert_eq!(
        outcomes,
        [
            Outcome::Changed("runs/suite/v1.subjects"),
            Outcome::Changed("runs/suite/v1.subjects"),
            Outcome::Unmapped("runs/suite/v1.subjects", "Raycast Workspaces".into()),
        ]
    );
}

#[test]
fn a_task_owner_that_names_nobody_is_kept_and_reported_and_its_twin_is_mapped() {
    let before = json!({"$type": "tasks/task/v2", "owner": "lead-7f3a"});
    let (record, outcomes) = migrated(before.clone());
    assert_eq!(record, before);
    assert_eq!(
        outcomes,
        [Outcome::Unmapped("tasks/task/v2.owner", "lead-7f3a".into())]
    );
    let (record, _) = migrated(json!({"$type": "tasks/task/v2", "owner": "codex lead"}));
    assert_eq!(record["owner"], json!({"program": "codex"}));
}

#[test]
fn a_version_session_the_released_mod_wrote_becomes_the_mods_program() {
    let (record, _) =
        migrated(json!({"$type": "artifacts/version/v1", "session": "0b8a7c6e-1f2d"}));
    assert_eq!(
        record,
        json!({"$type": "artifacts/version/v1", "by": {"program": "claude-code"}})
    );
}

#[test]
fn a_hash_that_is_neither_bare_hex_nor_a_digest_is_reported_and_its_twin_is_prefixed() {
    let (record, outcomes) = migrated(json!({"$type": "requests/reply/v1", "effect": "ab"}));
    assert_eq!(record["effect"], json!("ab"));
    assert_eq!(
        outcomes,
        [Outcome::Unmapped("requests/reply/v1.effect", "ab".into())]
    );
    let (seen, outcomes) = migrated(json!({"$type": "board/seen/v1", "effect": "/effects/x"}));
    assert_eq!(seen["effect"], json!("/effects/x"));
    assert_eq!(
        outcomes,
        [Outcome::Unmapped(
            "board/seen/v1.effect",
            "/effects/x".into()
        )]
    );
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
fn a_task_owner_line_in_frontmatter_becomes_an_actor_or_is_kept_and_reported() {
    let text = "---\n$type: \"tasks/task/v2\"\nowner: Codex integrator\ntitle: t\n---\nbody\n";
    let (migrated, outcomes) = rules::markdown(text, &context()).unwrap();
    assert_eq!(
        migrated,
        "---\n$type: \"tasks/task/v2\"\nowner: {\"program\":\"codex\"}\ntitle: t\n---\nbody\n"
    );
    assert_eq!(outcomes, [Outcome::Changed("tasks/task/v2.owner")]);
    let text = "---\n$type: tasks/task/v2\nowner: \"c2_b_builder\"\n---\n";
    let (kept, outcomes) = rules::markdown(text, &context()).unwrap();
    assert_eq!(kept, text);
    assert_eq!(
        outcomes,
        [Outcome::Unmapped(
            "tasks/task/v2.owner",
            "c2_b_builder".into()
        )]
    );
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
    assert_eq!(eval["subjects"], json!(["crumbkit/core", "outer"]));
    assert_eq!(
        report["unmapped"],
        json!([{"path": "/evals/e.json", "field": "evals/eval/v1.subjects", "value": "outer"}])
    );
}

/// Migrates a copy of the board `MIGRATE_BOARD` names, when it is set, and
/// checks that a second run changes nothing. It never writes the board itself.
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
    let first = run(&board, &options).unwrap();
    assert_ne!(first["files"]["read"], json!(0));
    let second = run(&board, &options).unwrap();
    assert_eq!(second["files"]["changed"], json!(0));
}
