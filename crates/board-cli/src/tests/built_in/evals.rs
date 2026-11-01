//! The evals plugin through the binary, with runs enabled beside it: what
//! `evals.about` and `evals.open` list, and what the shipped schema admits.
//! These were the plugin's pytest parity suite.

use super::*;

const INVALID: Option<&str> = Some("board::record::invalid");

/// A board that enables evals and runs.
fn board() -> Fixture {
    Fixture::enabling(&["evals", "runs"])
}

/// An `evals/eval/v1` record asking `question` about gix, with `fields`
/// added or replacing its own. Its status is derived from its fields.
fn eval(question: &str, fields: &Value) -> String {
    record(
        json!({
            "$type": "evals/eval/v1",
            "question": question,
            "subjects": ["gix"],
        }),
        fields,
    )
}

fn suite() -> String {
    json!({"$type": "runs/suite/v1", "subjects": ["gix"], "method": "Run it."}).to_string()
}

/// A run of `suite` with one passing result.
fn run(suite: &str) -> String {
    let run = json!({
        "$type": "runs/run/v1",
        "suite": suite,
        "by": "claude",
        "started": "2026-10-01T09:00:00Z",
        "scope": "full",
        "results": [{"case": "one", "outcome": "pass"}],
    });
    run.to_string()
}

/// The paths of the evals `evals.<capability>` lists with `flags`, in order.
fn paths(board: &Fixture, capability: &str, flags: &[&str]) -> Vec<String> {
    let output = board.call(&format!("evals.{capability}"), flags);
    (output["evals"].as_array().unwrap().iter())
        .map(|eval| eval["path"].as_str().unwrap().to_owned())
        .collect()
}

/// A repository whose board enables evals and runs, holding `files`.
fn evals_repository(files: &[(&str, String)]) -> (Scenario, ObjectId) {
    repository(&["evals", "runs"], files)
}

#[test]
fn about_lists_the_evals_that_name_a_subject_whatever_their_status_with_their_question_and_verdict()
{
    let board = board();
    board
        .write(
            "evals/planned.json",
            &eval("Planned?", &json!({"informs": "Pick a git library."})),
        )
        .write(
            "evals/done.json",
            &eval(
                "Done?",
                &json!({"subjects": ["gix", "git2"], "verdict": "supported", "conclusion": "Yes."}),
            ),
        )
        .write(
            "evals/other.json",
            &eval("Other?", &json!({"subjects": ["serde"]})),
        );
    let output = board.call("evals.about", &["--subject", "gix"]);
    assert_eq!(
        output,
        json!({
            "evals": [
                {
                    "path": "/evals/done",
                    "question": "Done?",
                    "subjects": ["gix", "git2"],
                    "status": "concluded",
                    "verdict": "supported",
                    "informs": null,
                    "decisions": [],
                },
                {
                    "path": "/evals/planned",
                    "question": "Planned?",
                    "subjects": ["gix"],
                    "status": "planned",
                    "verdict": null,
                    "informs": "Pick a git library.",
                    "decisions": [],
                },
            ],
            "total": 2,
            "truncated": false,
        })
    );
}

#[test]
fn about_matches_a_subject_without_regard_to_case_and_misses_a_different_subject() {
    let board = board();
    board.write(
        "evals/a.json",
        &eval("A?", &json!({"subjects": ["GitOxide"]})),
    );
    for asked in ["gitoxide", "GITOXIDE", "GitOxide"] {
        assert_eq!(paths(&board, "about", &["--subject", asked]), ["/evals/a"]);
    }
    for missed in ["git", "gitoxide2"] {
        assert!(paths(&board, "about", &["--subject", missed]).is_empty());
    }
}

#[test]
fn about_quotes_a_subject_so_it_matches_only_itself() {
    let board = board();
    board
        .write("evals/a.json", &eval("A?", &json!({"subjects": ["it's"]})))
        .write("evals/b.json", &eval("B?", &json!({"subjects": ["other"]})));
    assert_eq!(paths(&board, "about", &["--subject", "it's"]), ["/evals/a"]);
    assert!(paths(&board, "about", &["--subject", "x' OR '1'='1"]).is_empty());
}

#[test]
fn an_eval_that_is_removed_is_no_longer_listed_by_about_or_open() {
    let board = board();
    board.write("evals/a.json", &eval("A?", &json!({}))).write(
        "evals/b.json",
        &eval("B?", &json!({"runs": ["/suites/s/runs/1"]})),
    );
    assert_eq!(
        paths(&board, "about", &["--subject", "gix"]),
        ["/evals/a", "/evals/b"]
    );
    assert_eq!(paths(&board, "open", &[]), ["/evals/a", "/evals/b"]);
    board.delete("evals/a.json");
    assert_eq!(paths(&board, "about", &["--subject", "gix"]), ["/evals/b"]);
    assert_eq!(paths(&board, "open", &[]), ["/evals/b"]);
}

#[test]
fn a_record_of_another_type_or_one_the_schema_refuses_is_not_an_eval() {
    let board = board();
    board
        .write("evals/ok.json", &eval("Ok?", &json!({})))
        .write(
            "evals/stored-status.json",
            &eval("Stored?", &json!({"status": "planned"})),
        )
        // The plugin could read this one, which ignores fields it does not
        // know; Board's verdict leaves it out.
        .write(
            "evals/colour.json",
            &eval("Colour?", &json!({"colour": "red"})),
        )
        .write(
            "notes/stray.json",
            r#"{"$type": "other/thing/v1", "subjects": ["gix"]}"#,
        );
    assert_eq!(paths(&board, "about", &["--subject", "gix"]), ["/evals/ok"]);
}

#[test]
fn board_refuses_the_call_when_its_eval_schema_is_looser_and_answers_with_the_shipped_one() {
    let board = board();
    let ruled = json!({"verdict": "refuted"});
    board.write("evals/ruled.json", &eval("Ruled?", &ruled));
    assert_eq!(
        paths(&board, "about", &["--subject", "gix"]),
        ["/evals/ruled"]
    );
    let path = ".board/types/evals/eval/v1.schema.json";
    let mut loosened: Value = serde_json::from_str(&board.read(path)).unwrap();
    let schema = loosened.as_object_mut().unwrap();
    schema.remove("additionalProperties");
    board.write(path, &loosened.to_string());
    let refused = board.refusal("evals.about", &["--subject", "gix"]);
    assert_eq!(
        refused["code"], "board::plugin::schema_differs",
        "{refused}"
    );
    assert_eq!(refused["fields"]["schema"], path);
}

#[test]
fn an_eval_whose_run_reaches_nothing_is_still_listed() {
    let board = board();
    board
        .write("suites/s.json", &suite())
        .write("suites/s/runs/1.json", &run("/suites/s"))
        .write(
            "evals/ok.json",
            &eval("Ok?", &json!({"runs": ["/suites/s/runs/1"]})),
        )
        .write(
            "evals/lost.json",
            &eval("Lost?", &json!({"runs": ["/suites/s/runs/2"]})),
        );
    assert_eq!(
        paths(&board, "about", &["--subject", "gix"]),
        ["/evals/lost", "/evals/ok"]
    );
}

#[test]
fn about_and_open_answer_the_same_from_a_cold_cache() {
    let board = board();
    board
        .write(
            "evals/a.json",
            &eval("A?", &json!({"runs": ["/suites/s/runs/1"]})),
        )
        .write(
            "evals/b.json",
            &eval("B?", &json!({"verdict": "refuted", "conclusion": "No."})),
        )
        .write("evals/c.json", &eval("C?", &json!({"abandoned": "Moot."})))
        .write("evals/d.json", &eval("D?", &json!({})));
    for (target, flags) in [
        ("evals.about", &["--subject", "GIX"][..]),
        ("evals.open", &[]),
    ] {
        let warm = board.call(target, flags);
        assert_eq!(board.answer(target, flags, true), warm, "{target}");
    }
}

#[test]
fn about_and_open_show_at_most_their_limit_and_say_how_many_they_left_out() {
    let board = board();
    // Each eval names gix and the repo /repos/x, so all three cases list the
    // same three evals; the repo matters only to the `--repo` case.
    for name in ["a", "b", "c"] {
        board.write(
            &format!("evals/{name}.json"),
            &eval("Q?", &json!({"repos": ["/repos/x"]})),
        );
    }
    for (target, flags) in [
        ("evals.about", &["--subject", "gix"][..]),
        ("evals.about", &["--repo", "/repos/x"]),
        ("evals.open", &[]),
    ] {
        let with = |limit: &str| {
            let flags: Vec<&str> = flags.iter().copied().chain(["--limit", limit]).collect();
            board.call(target, &flags)
        };
        let cut = with("2");
        let listed: Vec<&Value> = cut["evals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|eval| &eval["path"])
            .collect();
        assert_eq!(listed, [&json!("/evals/a"), &json!("/evals/b")], "{target}");
        assert_eq!(
            (&cut["total"], &cut["truncated"]),
            (&json!(3), &json!(true))
        );
        let whole = with("3");
        assert_eq!(whole["evals"].as_array().unwrap().len(), 3);
        assert_eq!(
            (&whole["total"], &whole["truncated"]),
            (&json!(3), &json!(false))
        );
    }
}

#[test]
fn a_limit_outside_one_to_a_hundred_or_an_empty_subject_is_refused_by_board_before_the_plugin_runs()
{
    let board = board();
    for name in ["a", "b", "c"] {
        board.write(&format!("evals/{name}.json"), &eval("A?", &json!({})));
    }
    for (target, flags) in [
        ("evals.about", &["--subject", "gix"][..]),
        ("evals.open", &[]),
    ] {
        for limit in ["0", "101"] {
            let flags: Vec<&str> = flags.iter().copied().chain(["--limit", limit]).collect();
            let refused = board.refusal(target, &flags);
            assert_eq!(refused["code"], "board::plugin::bad_input", "{refused}");
        }
        for limit in ["1", "100"] {
            let flags: Vec<&str> = flags.iter().copied().chain(["--limit", limit]).collect();
            assert_eq!(board.call(target, &flags)["total"], 3, "{target} {limit}");
        }
    }
    let refused = board.refusal("evals.about", &["--subject", ""]);
    assert_eq!(refused["code"], "board::plugin::bad_input", "{refused}");
}

#[test]
fn about_answers_across_two_mounted_repositories_and_the_boards_own_evals() {
    let board = board();
    let (first, first_commit) = evals_repository(&[(
        "evals/pin-gix.json",
        eval("Does gix pin?", &json!({"informs": "Pick a library."})),
    )]);
    let (second, second_commit) = evals_repository(&[
        (
            "evals/clone-speed.json",
            eval("Is gix fast?", &json!({"subjects": ["gix", "git2"]})),
        ),
        (
            "evals/unrelated.json",
            eval("Is serde fast?", &json!({"subjects": ["serde"]})),
        ),
    ]);
    mount(&board, "repos/first.json", &first, first_commit);
    mount(&board, "repos/second.json", &second, second_commit);
    board.write("evals/local.json", &eval("Does gix build?", &json!({})));
    let everywhere = [
        "/evals/local",
        "/repos/first/evals/pin-gix",
        "/repos/second/evals/clone-speed",
    ];
    assert_eq!(paths(&board, "about", &["--subject", "gix"]), everywhere);
    assert_eq!(
        paths(&board, "about", &["--subject", "serde"]),
        ["/repos/second/evals/unrelated"]
    );
    let cold = board.answer("evals.about", &["--subject", "gix"], true);
    assert_eq!(cold, board.call("evals.about", &["--subject", "gix"]));
}

#[test]
fn a_mounted_eval_the_mounted_boards_schemas_refuse_is_not_listed_and_one_they_admit_is() {
    let board = board();
    // The plugin could read this one; the mounted board's schema refuses
    // its extra field.
    let (mounted, commit) = evals_repository(&[
        ("evals/good.json", eval("Good?", &json!({}))),
        (
            "evals/broken.json",
            eval("Broken?", &json!({"colour": "red"})),
        ),
    ]);
    mount(&board, "repos/repo.json", &mounted, commit);
    assert_eq!(
        paths(&board, "about", &["--subject", "gix"]),
        ["/repos/repo/evals/good"]
    );
}

#[test]
fn about_a_repo_lists_the_evals_that_name_its_repo_record_here_and_in_mounted_boards() {
    let board = board();
    let (crumbkit, commit) = evals_repository(&[(
        "evals/regression.json",
        eval(
            "Still green?",
            &json!({"subjects": ["crumbkit"], "repos": ["/repos/crumbkit"]}),
        ),
    )]);
    mount(&board, "repos/crumbkit.json", &crumbkit, commit);
    board
        .write(
            "evals/clap.json",
            &eval(
                "Does clap fit?",
                &json!({"subjects": ["clap"], "repos": ["/repos/crumbkit"], "informs": "Pick a parser."}),
            ),
        )
        .write("evals/named-only.json", &eval("Named only?", &json!({"subjects": ["crumbkit"]})))
        .write(
            "evals/other.json",
            &eval("Other repo?", &json!({"subjects": ["clap"], "repos": ["/repos/clap"]})),
        );
    let output = board.call("evals.about", &["--repo", "/repos/crumbkit"]);
    assert_eq!(
        output,
        json!({
            "evals": [
                {
                    "path": "/evals/clap",
                    "question": "Does clap fit?",
                    "subjects": ["clap"],
                    "status": "planned",
                    "verdict": null,
                    "informs": "Pick a parser.",
                    "decisions": [],
                },
                {
                    "path": "/repos/crumbkit/evals/regression",
                    "question": "Still green?",
                    "subjects": ["crumbkit"],
                    "status": "planned",
                    "verdict": null,
                    "informs": null,
                    "decisions": [],
                },
            ],
            "total": 2,
            "truncated": false,
        })
    );
    assert!(paths(&board, "about", &["--repo", "/repos/crumb"]).is_empty());
    assert!(paths(&board, "about", &["--subject", "/repos/crumbkit"]).is_empty());
    let cold = board.answer("evals.about", &["--repo", "/repos/crumbkit"], true);
    assert_eq!(cold, output);
}

#[test]
fn about_takes_exactly_one_of_subject_and_repo_and_board_refuses_anything_else_before_the_plugin_runs()
 {
    let board = board();
    let eval_a = eval("A?", &json!({"repos": ["/repos/x"]}));
    board.write("evals/a.json", &eval_a);
    let refused: [&[&str]; 4] = [
        &[],
        &["--limit", "5"],
        &["--subject", "gix", "--repo", "/repos/x"],
        &["--repo", ""],
    ];
    for flags in refused {
        let error = board.refusal("evals.about", flags);
        assert_eq!(
            error["code"], "board::plugin::bad_input",
            "{flags:?}: {error}"
        );
    }
    let unknown: [&[&str]; 2] = [
        &["--mount", "/repos/x"],
        &["--repo", "/repos/x", "--x", "1"],
    ];
    for flags in unknown {
        let error = board.refusal("evals.about", flags);
        assert_eq!(error["code"], "board::cli::usage", "{flags:?}: {error}");
    }
    let admitted: [&[&str]; 3] = [
        &["--subject", "gix"],
        &["--repo", "/repos/x"],
        &["--repo", "/repos/x", "--limit", "5"],
    ];
    for flags in admitted {
        assert_eq!(board.call("evals.about", flags)["total"], 1, "{flags:?}");
    }
}

#[test]
fn about_a_repo_lists_the_evals_that_live_in_its_mount_and_not_those_in_a_mount_sharing_its_prefix()
{
    let board = board();
    // Evals in a repository's own board leave `repos` out; they are found
    // by where they live.
    let (crumbkit, crumbkit_commit) = evals_repository(&[
        (
            "evals/own.json",
            eval("Own?", &json!({"subjects": ["crumbkit"]})),
        ),
        (
            "evals/names-clap.json",
            eval(
                "Names clap?",
                &json!({"subjects": ["crumbkit"], "repos": ["/repos/clap"]}),
            ),
        ),
    ]);
    let (sibling, sibling_commit) =
        evals_repository(&[("evals/near.json", eval("Near?", &json!({})))]);
    let (wild, wild_commit) = evals_repository(&[("evals/wild.json", eval("Wild?", &json!({})))]);
    mount(&board, "repos/crumbkit.json", &crumbkit, crumbkit_commit);
    mount(
        &board,
        "repos/crumbkit-extra.json",
        &sibling,
        sibling_commit,
    );
    mount(&board, "repos/crumb_it.json", &wild, wild_commit);
    let inside = [
        "/repos/crumbkit/evals/names-clap",
        "/repos/crumbkit/evals/own",
    ];
    assert_eq!(
        paths(&board, "about", &["--repo", "/repos/crumbkit"]),
        inside
    );
    for (mount, expected) in [
        (
            "/repos/crumbkit-extra",
            &["/repos/crumbkit-extra/evals/near"][..],
        ),
        ("/repos/crumb", &[]),
        ("/repos/crumb_it", &["/repos/crumb_it/evals/wild"]),
        ("/repos/crumb%", &[]),
        ("/repos/crumbkit/evals/own", &[]),
    ] {
        assert_eq!(
            paths(&board, "about", &["--repo", mount]),
            expected,
            "{mount}"
        );
    }
    let cold = board.answer("evals.about", &["--repo", "/repos/crumbkit"], true);
    assert_eq!(
        cold,
        board.call("evals.about", &["--repo", "/repos/crumbkit"])
    );
}

#[test]
fn about_a_repo_counts_and_limits_the_evals_in_its_mount_with_those_that_name_it() {
    let board = board();
    let (mounted, commit) = evals_repository(&[
        ("evals/b.json", eval("B?", &json!({}))),
        ("evals/c.json", eval("C?", &json!({"repos": ["/repos/x"]}))),
    ]);
    mount(&board, "repos/x.json", &mounted, commit);
    board.write("evals/a.json", &eval("A?", &json!({"repos": ["/repos/x"]})));
    let cut = board.call("evals.about", &["--repo", "/repos/x", "--limit", "2"]);
    let listed: Vec<&Value> = cut["evals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|eval| &eval["path"])
        .collect();
    assert_eq!(listed, [&json!("/evals/a"), &json!("/repos/x/evals/b")]);
    assert_eq!(
        (&cut["total"], &cut["truncated"]),
        (&json!(3), &json!(true))
    );
}

#[test]
fn open_lists_planned_and_running_evals_and_leaves_out_concluded_and_abandoned_ones() {
    let board = board();
    board
        .write(
            "evals/b-running.json",
            &eval(
                "B?",
                &json!({"subjects": ["serde"], "runs": ["/suites/s/runs/1"]}),
            ),
        )
        .write(
            "evals/a-planned.json",
            &eval("A?", &json!({"informs": "Pick a library."})),
        )
        .write(
            "evals/done.json",
            &eval(
                "Done?",
                &json!({"verdict": "supported", "conclusion": "Yes."}),
            ),
        )
        .write(
            "evals/dropped.json",
            &eval("Dropped?", &json!({"abandoned": "Moot."})),
        );
    let output = board.call("evals.open", &[]);
    assert_eq!(
        output,
        json!({
            "evals": [
                {
                    "path": "/evals/a-planned",
                    "question": "A?",
                    "subjects": ["gix"],
                    "status": "planned",
                    "verdict": null,
                    "informs": "Pick a library.",
                    "decisions": [],
                },
                {
                    "path": "/evals/b-running",
                    "question": "B?",
                    "subjects": ["serde"],
                    "status": "running",
                    "verdict": null,
                    "informs": null,
                    "decisions": [],
                },
            ],
            "total": 2,
            "truncated": false,
        })
    );
}

#[test]
fn an_eval_that_concludes_leaves_the_open_list() {
    let board = board();
    board.write(
        "evals/a.json",
        &eval("A?", &json!({"runs": ["/suites/s/runs/1"]})),
    );
    assert_eq!(paths(&board, "open", &[]), ["/evals/a"]);
    let concluded = json!({"verdict": "refuted", "conclusion": "No."});
    board.write("evals/a.json", &eval("A?", &concluded));
    assert!(paths(&board, "open", &[]).is_empty());
}

#[test]
fn open_lists_the_open_evals_of_two_mounted_repositories() {
    let board = board();
    let (first, first_commit) = evals_repository(&[
        ("evals/a.json", eval("A?", &json!({}))),
        (
            "evals/done.json",
            eval("D?", &json!({"abandoned": "Moot."})),
        ),
    ]);
    let (second, second_commit) = evals_repository(&[(
        "evals/b.json",
        eval("B?", &json!({"runs": ["/suites/s/runs/1"]})),
    )]);
    mount(&board, "repos/first.json", &first, first_commit);
    mount(&board, "repos/second.json", &second, second_commit);
    assert_eq!(
        paths(&board, "open", &[]),
        ["/repos/first/evals/a", "/repos/second/evals/b"]
    );
}

#[test]
fn board_lists_evals_as_enabled_and_enabling_wrote_the_schema_it_ships() {
    let board = board();
    let report = board.json(&["plugin", "list"]);
    let plugin = (report["data"].as_array().unwrap().iter())
        .find(|plugin| plugin["name"] == "evals")
        .unwrap();
    assert_eq!(
        (&plugin["status"], &plugin["enabled"]),
        (&json!("ok"), &json!(true))
    );
    assert_eq!(plugin.get("run"), None);
    let shipped = shipped("evals");
    assert_eq!(shipped.len(), 1);
    for (path, text) in shipped {
        assert_eq!(board.read(&path), text, "{path}");
    }
}

#[test]
fn a_stored_status_is_refused_and_an_eval_without_one_is_admitted() {
    let board = board();
    board
        .write("evals/derived.json", &eval("D?", &json!({})))
        .write(
            "evals/stored.json",
            &eval("S?", &json!({"status": "planned"})),
        );
    assert_eq!(
        board.checked("/evals/"),
        codes(&[("/evals/derived", None), ("/evals/stored", INVALID)])
    );
}

#[test]
fn a_verdict_and_an_abandoned_reason_are_admitted_and_an_unknown_verdict_or_an_empty_reason_is_not()
{
    let board = board();
    board
        .write(
            "evals/with.json",
            &eval("W?", &json!({"verdict": "refuted"})),
        )
        .write("evals/odd.json", &eval("O?", &json!({"verdict": "maybe"})))
        .write(
            "evals/dropped.json",
            &eval("D?", &json!({"abandoned": "The library was withdrawn."})),
        )
        .write("evals/empty.json", &eval("E?", &json!({"abandoned": ""})));
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/with", None),
            ("/evals/odd", INVALID),
            ("/evals/dropped", None),
            ("/evals/empty", INVALID),
        ])
    );
}

#[test]
fn an_eval_needs_a_question_and_subjects() {
    let board = board();
    board.write("evals/whole.json", &eval("W?", &json!({})));
    for missing in ["question", "subjects"] {
        let mut record: Value = serde_json::from_str(&eval("M?", &json!({}))).unwrap();
        record.as_object_mut().unwrap().remove(missing);
        board.write(&format!("evals/no-{missing}.json"), &record.to_string());
    }
    board.write(
        "evals/no-subjects-listed.json",
        &eval("N?", &json!({"subjects": []})),
    );
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/whole", None),
            ("/evals/no-question", INVALID),
            ("/evals/no-subjects", INVALID),
            ("/evals/no-subjects-listed", INVALID),
        ])
    );
}

#[test]
fn an_eval_refers_to_suites_and_runs_and_holds_no_other_field() {
    let board = board();
    let full = json!({"suites": ["/suites/s"], "runs": ["/suites/s/runs/1"], "informs": "Pick."});
    board
        .write("suites/s.json", &suite())
        .write("suites/s/runs/1.json", &run("/suites/s"))
        .write("evals/full.json", &eval("F?", &full))
        .write(
            "evals/method.json",
            &eval("M?", &json!({"method": "Run it."})),
        )
        .write("evals/colour.json", &eval("C?", &json!({"colour": "red"})));
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/full", None),
            ("/evals/method", INVALID),
            ("/evals/colour", INVALID),
        ])
    );
}

#[test]
fn an_eval_whose_suite_or_run_is_the_wrong_type_or_missing_is_degraded() {
    let board = board();
    let right = json!({"suites": ["/suites/s"], "runs": ["/suites/s/runs/1"]});
    board
        .write("suites/s.json", &suite())
        .write("suites/s/runs/1.json", &run("/suites/s"))
        .write("evals/right.json", &eval("R?", &right))
        .write(
            "evals/suite-is-run.json",
            &eval("S?", &json!({"suites": ["/suites/s/runs/1"]})),
        )
        .write(
            "evals/run-is-suite.json",
            &eval("U?", &json!({"runs": ["/suites/s"]})),
        )
        .write(
            "evals/run-gone.json",
            &eval("G?", &json!({"runs": ["/suites/s/runs/2"]})),
        );
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/right", None),
            ("/evals/suite-is-run", Some("board::ref::wrong_type")),
            ("/evals/run-is-suite", Some("board::ref::wrong_type")),
            ("/evals/run-gone", Some("board::ref::dangling")),
        ])
    );
}

#[test]
fn an_eval_names_each_repository_by_its_repo_record_and_one_the_board_does_not_have_is_dangling() {
    let board = board();
    let (crumbkit, commit) = evals_repository(&[]);
    mount(&board, "repos/crumbkit.json", &crumbkit, commit);
    let named = |fields: Value| {
        let mut fields = fields;
        fields["subjects"] = json!(["crumbkit"]);
        eval("Q?", &fields)
    };
    board
        .write(
            "repos/clap.json",
            &json!({"$type": "board/repo/v1", "remote": "clap-rs/clap"}).to_string(),
        )
        .write(
            "evals/identity.json",
            &eval(
                "I?",
                &json!({"subjects": ["clap"], "repos": ["/repos/clap"]}),
            ),
        )
        .write(
            "evals/mounted.json",
            &named(json!({"repos": ["/repos/crumbkit"]})),
        )
        .write("evals/none.json", &named(json!({})))
        .write(
            "evals/missing.json",
            &named(json!({"repos": ["/repos/crumbkit", "/repos/gone"]})),
        )
        .write(
            "evals/by-name.json",
            &named(json!({"repos": ["crumbkit/crumbkit"]})),
        )
        .write(
            "evals/not-a-list.json",
            &named(json!({"repos": "/repos/crumbkit"})),
        )
        .write(
            "evals/old-key.json",
            &named(json!({"mounts": ["/repos/crumbkit"]})),
        );
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/mounted", None),
            ("/evals/identity", None),
            ("/evals/none", None),
            ("/evals/missing", Some("board::ref::dangling")),
            ("/evals/by-name", Some("board::ref::malformed")),
            ("/evals/not-a-list", INVALID),
            ("/evals/old-key", INVALID),
        ])
    );
}

#[test]
fn board_refs_on_a_repositorys_mount_lists_the_evals_that_name_it_and_no_others() {
    let board = board();
    let mut repositories = Vec::new();
    for name in ["crumbkit", "clap"] {
        // Each holds a file of its own; two identical repositories would
        // share one commit and be one object.
        let (root, commit) = evals_repository(&[("README.md", name.to_owned())]);
        mount(&board, &format!("repos/{name}.json"), &root, commit);
        repositories.push(root);
    }
    board
        .write(
            "evals/a.json",
            &eval("A?", &json!({"repos": ["/repos/crumbkit"]})),
        )
        .write(
            "evals/b.json",
            &eval("B?", &json!({"repos": ["/repos/clap", "/repos/crumbkit"]})),
        )
        .write(
            "evals/c.json",
            &eval("C?", &json!({"repos": ["/repos/clap"]})),
        )
        .write("evals/d.json", &eval("D?", &json!({})));
    let naming = |path: &str| {
        let report = board.json(&["refs", path]);
        let mut links: Vec<(String, Value)> = (report["data"][0]["to"].as_array().unwrap().iter())
            .map(|link| {
                (
                    link["logical"].as_str().unwrap().to_owned(),
                    link["pointers"].clone(),
                )
            })
            .collect();
        links.sort_by(|a, b| a.0.cmp(&b.0));
        links
    };
    let link = |path: &str, pointer: &str| (path.to_owned(), json!([pointer]));
    assert_eq!(
        naming("/repos/crumbkit"),
        [link("/evals/a", "/repos/0"), link("/evals/b", "/repos/1")]
    );
    assert_eq!(
        naming("/repos/clap"),
        [link("/evals/b", "/repos/0"), link("/evals/c", "/repos/0")]
    );
}

#[test]
fn an_eval_a_hook_wrote_names_the_plugin_and_hook_and_nothing_else() {
    let board = board();
    board
        .write(
            "evals/hooked.json",
            &eval("H?", &json!({"written_by": "runs/test-run"})),
        )
        .write(
            "evals/prose.json",
            &eval("P?", &json!({"written_by": "the test hook"})),
        )
        .write("evals/empty.json", &eval("E?", &json!({"written_by": ""})));
    assert_eq!(
        board.checked("/evals/"),
        codes(&[
            ("/evals/hooked", None),
            ("/evals/prose", INVALID),
            ("/evals/empty", INVALID),
        ])
    );
}
