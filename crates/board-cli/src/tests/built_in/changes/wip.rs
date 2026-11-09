//! `test_wip.py`: the soft WIP limit on active changes in build per
//! repository, and the paused phase that holds a change outside it.

use super::*;

const CHANGE: &str = "/changes/t";
/// The changes of one kind that may be active in build per repository.
const WIP_LIMIT: usize = 3;
/// An active-days window wide enough to hold changes touched ten days ago.
const WIDE: u64 = 30;

fn path() -> Value {
    json!({"path": CHANGE})
}

/// `{"path": CHANGE}` with `fields`.
fn at(fields: &Value) -> Value {
    serde_json::from_str(&record(path(), fields)).unwrap()
}

/// `count` other changes in build, with `fields`.
fn fill(board: &Changes, count: usize, fields: &Value) {
    for number in 0..count {
        board.change_at(&format!("changes/busy-{number}.json"), "build", fields);
    }
}

/// The paths of the changes [`fill`] wrote.
fn busy(count: usize) -> Value {
    (0..count)
        .map(|number| format!("/changes/busy-{number}"))
        .collect()
}

/// A change in plan whose gate passes.
fn ready(board: &Changes) {
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    board.write("changes/t/plan.md", PLAN);
}

fn advanced(board: &Changes) -> Value {
    board.call("advance", &path())["to"].clone()
}

#[test]
fn advance_into_build_refuses_when_the_cap_is_full_and_moves_on_below_it() {
    let board = Changes::new();
    ready(&board);
    fill(&board, WIP_LIMIT, &json!({}));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::wip::full");
    assert_eq!(refused["fields"]["active"], busy(WIP_LIMIT));
    board.call("pause", &json!({"path": "/changes/busy-0"}));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn start_on_a_flow_that_begins_in_build_refuses_a_full_cap_and_goes_past_it_with_a_reason() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    fill(&board, WIP_LIMIT, &json!({}));
    let input = json!({"mount": "/repos/x", "task": TASK, "title": "Quick", "flow": "small"});
    let refused = board.refusal("start", &input);
    let override_command = format!(
        "board changes start --mount /repos/x --title Quick --task {TASK} --flow small \
         --over_limit '<why>'"
    );
    assert_eq!(
        (&refused["code"], &refused["fields"]["override"]),
        (&json!("changes::wip::full"), &json!(override_command))
    );
    assert!(!board.exists("changes/quick.json"));
    let reason = "The owner wants it now.";
    let given = serde_json::from_str(&record(input, &json!({"over_limit": reason}))).unwrap();
    let output = board.call("start", &given);
    assert_eq!(
        (&output["phase"], &output["warning"]["code"]),
        (&json!("build"), &json!("changes::wip::over_limit"))
    );
    assert_eq!(board.read("changes/quick.json")["over_limit"], reason);
}

#[test]
fn prototypes_exploring_do_not_fill_the_build_cap() {
    let board = Changes::new();
    ready(&board);
    fill(
        &board,
        WIP_LIMIT,
        &json!({"kind": "prototype", "state": "exploring"}),
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn pause_moves_a_change_in_build_to_paused_and_refuses_one_in_another_phase() {
    let board = Changes::new();
    board.change_at("changes/t.json", "review", &json!({}));
    assert_eq!(
        board.refusal("pause", &path())["code"],
        "changes::pause::not_building"
    );
    board.change_at("changes/t.json", "build", &json!({}));
    assert_eq!(
        board.call("pause", &path()),
        json!({"path": CHANGE, "from": "build", "to": "paused"})
    );
    assert_eq!(
        board.call("open", &json!({}))["changes"][0]["phase"],
        "paused"
    );
}

#[test]
fn a_paused_change_does_not_advance_and_says_to_resume() {
    let board = Changes::new();
    board.change_at("changes/t.json", "paused", &json!({}));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::phase::paused"
    );
    let output = board.call("next", &path());
    let instruction = output["instruction"].as_str().unwrap();
    assert_eq!(
        instruction,
        "This change was paused to free its place under the WIP limit on active changes of its \
         kind per repository. Resume it with `board changes resume --path /changes/t` when \
         a place is free and the owner's queue allows."
    );
    assert!(!instruction.contains("keeper"), "{instruction}");
    board.change_at("changes/t.json", "build", &json!({}));
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::gate::unmet"
    );
}

#[test]
fn resume_refuses_when_the_cap_is_full_and_moves_the_change_back_to_build_below_it() {
    let board = Changes::new();
    board.change_at("changes/t.json", "paused", &json!({}));
    fill(&board, WIP_LIMIT, &json!({}));
    assert_eq!(
        board.refusal("resume", &path())["code"],
        "changes::wip::full"
    );
    board.delete("changes/busy-0.json");
    assert_eq!(
        board.call("resume", &path()),
        json!({"path": CHANGE, "from": "paused", "to": "build"})
    );
}

#[test]
fn resume_refuses_a_change_that_is_not_paused_and_admits_one_that_is() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({}));
    assert_eq!(
        board.refusal("resume", &path())["code"],
        "changes::resume::not_paused"
    );
    board.change_at("changes/t.json", "paused", &json!({}));
    assert_eq!(board.call("resume", &path())["to"], "build");
}

#[test]
fn advance_past_the_limit_with_a_reason_goes_ahead_records_it_and_warns() {
    let board = Changes::new();
    ready(&board);
    fill(&board, WIP_LIMIT, &json!({}));
    let refused = board.refusal("advance", &path());
    let override_command = format!("board changes advance --path {CHANGE} --over_limit '<why>'");
    assert_eq!(
        (&refused["code"], &refused["fields"]["override"]),
        (&json!("changes::wip::full"), &json!(override_command))
    );
    let output = board.call(
        "advance",
        &at(&json!({"over_limit": "The owner wants this before the others."})),
    );
    assert_eq!(output["to"], "build");
    let warning = &output["warning"];
    assert_eq!(
        (
            &warning["code"],
            &warning["mount"],
            &warning["limit"],
            &warning["active_days"]
        ),
        (
            &json!("changes::wip::over_limit"),
            &json!("/repos/x"),
            &json!(WIP_LIMIT),
            &json!(3)
        )
    );
    assert_eq!(warning["active"], busy(WIP_LIMIT));
    let recorded = board.read("changes/t.json");
    assert_eq!(
        (&recorded["over_limit"], &recorded["touched"]),
        (
            &json!("The owner wants this before the others."),
            &json!(today())
        )
    );
}

/// Three changes in build touched ten days ago: one with a pull request,
/// one with a head, one with neither.
fn idle(board: &Changes) {
    let touched = days_ago(10);
    board.change_at(
        "changes/idle-pr.json",
        "build",
        &json!({"touched": touched, "pull_request": "https://github.com/o/r/pull/1"}),
    );
    board.change_at(
        "changes/idle-head.json",
        "build",
        &json!({"touched": touched, "head": "a3f9c0d"}),
    );
    board.change_at(
        "changes/idle-bare.json",
        "build",
        &json!({"touched": touched}),
    );
}

#[test]
fn idle_changes_do_not_count_and_the_refusal_lists_each_with_its_action() {
    let board = Changes::new();
    ready(&board);
    idle(&board);
    assert_eq!(advanced(&board), "build");
    board.change_at(
        "changes/t.json",
        "plan",
        &json!({"plan": "/changes/t/plan.md", "route": "owner", "records": [board.approved()]}),
    );
    fill(&board, WIP_LIMIT, &json!({}));
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::wip::full");
    assert_eq!(
        refused["fields"]["idle"],
        json!([
            {
                "path": "/changes/idle-bare",
                "action": "drop",
                "command": "board changes drop --path /changes/idle-bare --reason '<why>'",
            },
            {
                "path": "/changes/idle-head",
                "action": "resume",
                "command": "board changes next --path /changes/idle-head",
            },
            {"path": "/changes/idle-pr", "action": "sync", "command": "board changes sync"},
        ])
    );
}

#[test]
fn the_limit_counts_only_changes_for_the_same_mount() {
    let board = Changes::new();
    ready(&board);
    fill(&board, WIP_LIMIT, &json!({"mount": "/repos/y"}));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn a_run_that_finished_today_keeps_an_untouched_change_active() {
    let board = Changes::new();
    ready(&board);
    board.write(
        "suites/s/runs/r.json",
        &typed(
            "runs/run/v1",
            &json!({"finished": format!("{}T09:00:00Z", today())}),
        ),
    );
    fill(&board, WIP_LIMIT - 1, &json!({}));
    board.change_at(
        "changes/ran.json",
        "build",
        &json!({"touched": days_ago(10), "records": ["/suites/s/runs/r"]}),
    );
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::wip::full"
    );
    board.write(
        "suites/s/runs/r.json",
        &typed(
            "runs/run/v1",
            &json!({"finished": format!("{}T09:00:00Z", days_ago(10))}),
        ),
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn a_head_committed_today_keeps_an_untouched_change_active() {
    let board = Changes::new();
    ready(&board);
    board.mount_x(&[("README.md", "x\n")]);
    let head = board.commit(&[("a.txt", "a\n")], None);
    fill(&board, WIP_LIMIT - 1, &json!({}));
    board.change_at(
        "changes/committed.json",
        "build",
        &json!({"touched": days_ago(10), "head": head}),
    );
    assert_eq!(
        board.refusal("advance", &path())["code"],
        "changes::wip::full"
    );
    board.change_at(
        "changes/committed.json",
        "build",
        &json!({"touched": days_ago(10), "head": "b".repeat(40)}),
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn board_changes_active_days_widens_the_window_and_refuses_a_value_that_is_not_a_number_of_days() {
    let board = Changes::new();
    ready(&board);
    fill(&board, WIP_LIMIT, &json!({"touched": days_ago(10)}));
    let wide = WIDE.to_string();
    let refused =
        board.refusal_inheriting("advance", &path(), &[("BOARD_CHANGES_ACTIVE_DAYS", &wide)]);
    assert_eq!(refused["fields"]["active_days"], WIDE);
    let refused =
        board.refusal_inheriting("advance", &path(), &[("BOARD_CHANGES_ACTIVE_DAYS", "soon")]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["value"]),
        (&json!("changes::wip::bad_active_days"), &json!("soon"))
    );
    assert_eq!(advanced(&board), "build");
}

#[test]
fn resume_past_the_limit_with_a_reason_goes_ahead_and_warns() {
    let board = Changes::new();
    board.change_at("changes/t.json", "paused", &json!({}));
    fill(&board, WIP_LIMIT, &json!({}));
    let output = board.call(
        "resume",
        &at(&json!({"over_limit": "Blocked work elsewhere."})),
    );
    assert_eq!(
        (&output["to"], &output["warning"]["code"]),
        (&json!("build"), &json!("changes::wip::over_limit"))
    );
    assert_eq!(
        board.read("changes/t.json")["over_limit"],
        "Blocked work elsewhere."
    );
}

#[test]
fn attach_touches_a_change_so_it_counts_as_active_again() {
    let board = Changes::new();
    board.change_at("changes/t.json", "build", &json!({"touched": days_ago(10)}));
    board.call("attach", &at(&json!({"head": "a3f9c0d"})));
    assert_eq!(board.read("changes/t.json")["touched"], today());
}

impl Changes {
    /// A clone mounted at `/<at>` whose origin is `origin`, or a clone with
    /// no origin: its mount.
    fn repository(&self, at: &str, origin: Option<&str>) -> String {
        let mount = self.mount_repo(&[("README.md", "x\n")], at, "HEAD");
        if let Some(origin) = origin {
            git(&self.repo_root(at), &["remote", "add", "origin", origin]);
        }
        mount
    }

    /// `count` active changes in build for the repository at `mount`,
    /// named `<prefix>-<n>`: their paths.
    fn building_in(&self, mount: &str, count: usize, prefix: &str) -> Vec<String> {
        (0..count)
            .map(|number| {
                self.change_at(
                    &format!("changes/{prefix}-{number}.json"),
                    "build",
                    &json!({"mount": mount}),
                );
                format!("/changes/{prefix}-{number}")
            })
            .collect()
    }

    /// A change in plan for the repository at `mount` whose gate passes.
    fn ready_in(&self, mount: &str) {
        self.change_at(
            "changes/t.json",
            "plan",
            &json!({
                "plan": "/changes/t/plan.md", "route": "owner", "mount": mount,
                "records": [self.approved()],
            }),
        );
        self.write("changes/t/plan.md", PLAN);
    }

    /// A plain git repository at `at` beside the board, whose origin is
    /// `origin`: its path.
    fn clone_of(&self, at: &str, origin: &str) -> String {
        let root = self.beside.join(at);
        make_dir(&root);
        git(&root, &["init", "-q"]);
        git(&root, &["remote", "add", "origin", origin]);
        root.to_str().unwrap().to_owned()
    }
}

#[test]
fn mounts_of_one_repository_count_together_however_its_origin_is_written() {
    let board = Changes::new();
    let first = board.repository("repos/a", Some("https://github.com/crumbkit/a.git"));
    let second = board.repository("repos/a-again", Some("git@github.com:crumbkit/a.git"));
    let third = board.repository("repos/a-www", Some("https://www.github.com/Crumbkit/A"));
    let fourth = board.repository(
        "repos/a-port",
        Some("ssh://git@github.com:22/crumbkit/a.git"),
    );
    let other = board.repository("repos/c", Some("git@github.com:crumbkit/c.git"));
    let mut holding = board.building_in(&first, 1, "a");
    holding.extend(board.building_in(&second, 1, "b"));
    holding.extend(board.building_in(&third, 1, "c"));
    board.ready_in(&fourth);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::wip::full");
    assert_eq!(
        (
            &refused["fields"]["repository"],
            &refused["fields"]["mount"]
        ),
        (&json!("crumbkit/a"), &json!(fourth))
    );
    assert_eq!(refused["fields"]["active"], json!(holding));
    assert_eq!(refused["fields"]["unread"], json!({}));
    board.ready_in(&other);
    assert_eq!(advanced(&board), "build");
}

#[test]
fn a_clone_whose_origin_git_cannot_read_counts_by_its_clone_and_the_refusal_says_why() {
    let board = Changes::new();
    let a = board.repository("repos/a", None);
    board.building_in(&a, WIP_LIMIT, "a");
    board.ready_in(&a);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::wip::full");
    assert_eq!(
        refused["fields"]["repository"],
        board.repo_root("repos/a").to_str().unwrap()
    );
    let unread = refused["fields"]["unread"].as_object().unwrap();
    assert_eq!(unread.keys().collect::<Vec<_>>(), ["/repos/a"]);
    let why = unread["/repos/a"].as_str().unwrap();
    assert!(why.contains("origin"), "{why}");
    board.ready_in(&board.repository("repos/b", None));
    assert_eq!(advanced(&board), "build");
}

#[test]
fn the_over_limit_warning_names_the_repository_it_counted() {
    let board = Changes::new();
    let a = board.repository("repos/a", Some("https://github.com/crumbkit/a.git"));
    let again = board.repository("repos/a-again", Some("git@github.com:crumbkit/a.git"));
    board.building_in(&a, WIP_LIMIT, "a");
    board.ready_in(&again);
    let output = board.call(
        "advance",
        &at(&json!({"over_limit": "The owner asked for it."})),
    );
    let warning = &output["warning"];
    assert_eq!(
        (&warning["code"], &warning["repository"], &warning["mount"]),
        (
            &json!("changes::wip::over_limit"),
            &json!("crumbkit/a"),
            &json!(again)
        )
    );
}

#[test]
fn prototypes_of_one_repository_count_together_apart_from_build_changes() {
    let board = Changes::new();
    let a = board.repository("repos/a", Some("https://github.com/crumbkit/a.git"));
    let again = board.repository("repos/a-again", Some("git@github.com:crumbkit/a.git"));
    board.building_in(&a, WIP_LIMIT, "a");
    for number in 0..WIP_LIMIT {
        board.change_at(
            &format!("changes/p-{number}.json"),
            "build",
            &json!({"mount": a, "kind": "prototype", "state": "exploring"}),
        );
    }
    board.change_at(
        "changes/t.json",
        "paused",
        &json!({"mount": again, "kind": "prototype", "state": "exploring"}),
    );
    let refused = board.refusal("resume", &path());
    assert_eq!(
        (&refused["code"], &refused["fields"]["kind"]),
        (&json!("changes::wip::full"), &json!("prototype"))
    );
    let prototypes: Value = (0..WIP_LIMIT)
        .map(|number| format!("/changes/p-{number}"))
        .collect();
    assert_eq!(refused["fields"]["active"], prototypes);
    let b = board.repository("repos/b", None);
    board.change_at(
        "changes/t.json",
        "paused",
        &json!({"mount": b, "kind": "prototype"}),
    );
    assert_eq!(board.call("resume", &path())["to"], "build");
}

#[test]
fn a_clone_of_a_local_clone_counts_with_the_repository_that_clone_follows() {
    for prefix in ["", "file://"] {
        let board = Changes::new();
        let a = board.repository("repos/a", Some("https://github.com/crumbkit/a.git"));
        let followed = format!("{prefix}{}", board.repo_root("repos/a").display());
        let candidate = board.repository("repos/a-candidate", Some(&followed));
        let holding = board.building_in(&a, WIP_LIMIT, "a");
        board.ready_in(&candidate);
        let refused = board.refusal("advance", &path());
        assert_eq!(refused["code"], "changes::wip::full", "{prefix}");
        assert_eq!(
            (
                &refused["fields"]["repository"],
                &refused["fields"]["active"]
            ),
            (&json!("crumbkit/a"), &json!(holding)),
            "{prefix}"
        );
        assert_eq!(refused["fields"]["unread"], json!({}), "{prefix}");
        let b = format!(
            "{prefix}{}",
            board.clone_of("b", "https://github.com/crumbkit/b.git")
        );
        let other = board.repository("repos/b-candidate", Some(&b));
        board.ready_in(&other);
        assert_eq!(advanced(&board), "build", "{prefix}");
    }
}

#[test]
fn a_chain_of_local_clones_past_three_hops_counts_by_the_last_and_says_so() {
    let board = Changes::new();
    let end = board.clone_of("end", "https://github.com/crumbkit/a.git");
    let third = board.clone_of("third", &end);
    let second = board.clone_of("second", &third);
    let first = board.clone_of("first", &second);
    let long = board.repository("repos/long", Some(&first));
    board.building_in(&long, WIP_LIMIT, "l");
    board.ready_in(&long);
    let refused = board.refusal("advance", &path());
    assert_eq!(refused["code"], "changes::wip::full");
    assert_eq!(refused["fields"]["repository"], end);
    let why = refused["fields"]["unread"]["/repos/long"].as_str().unwrap();
    assert!(why.contains('3'), "{why}");
    let short = board.repository("repos/short", Some(&second));
    board.ready_in(&short);
    assert_eq!(advanced(&board), "build");
}

#[test]
fn reopen_refuses_when_the_cap_is_full_and_goes_past_it_with_a_reason() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.change_at("changes/t.json", "review", &json!({}));
    board.call("migrate", &json!({"apply": true}));
    fill(&board, WIP_LIMIT, &json!({}));
    let by_person = json!({"reason": "The review's fix.", "person": "luke"});
    let refused = board.refusal("reopen", &at(&by_person));
    assert_eq!(
        (&refused["code"], &refused["fields"]["override"]),
        (
            &json!("changes::wip::full"),
            &json!(format!(
                "board changes reopen --path {CHANGE} --reason 'The review'\"'\"'s fix.' \
                 --person luke --over_limit '<why>'"
            ))
        )
    );
    let reason = json!({"reason": "The review asked for a fix.", "agent": "lead"});
    let refused = board.refusal("reopen", &at(&reason));
    let override_command = format!(
        "board changes reopen --path {CHANGE} --reason 'The review asked for a fix.' \
         --agent lead --over_limit '<why>'"
    );
    assert_eq!(
        (&refused["code"], &refused["fields"]["override"]),
        (&json!("changes::wip::full"), &json!(override_command))
    );
    let past = json!({
        "reason": "The review asked for a fix.",
        "agent": "lead",
        "over_limit": "Blocked work elsewhere.",
    });
    let output = board.call("reopen", &at(&past));
    assert_eq!(
        (&output["to"], &output["warning"]["code"]),
        (&json!("build"), &json!("changes::wip::over_limit"))
    );
    assert_eq!(output["warning"]["override"], json!(override_command));
    assert_eq!(
        board.read("changes/t.json")["over_limit"],
        "Blocked work elsewhere."
    );
}

/// Each way a mount at `<at>` cannot be read: with the repository's `.git`
/// gone, and with a commit the repository lacks.
#[expect(
    clippy::disallowed_methods,
    reason = "the test removes the mounted repository's git directory, as happens to a mount"
)]
fn break_mount(board: &Changes, at: &str, losing_git: bool) {
    if losing_git {
        std::fs::remove_dir_all(board.repo_root(at).join(".git")).unwrap();
    } else {
        let file = format!("{at}.json");
        let mut pointer = board.read(&file);
        pointer["commit"] = json!("9".repeat(40));
        board.write(&file, &pointer.to_string());
    }
}

#[test]
fn another_changes_mount_that_cannot_be_read_warns_and_every_start_carries_on() {
    for (losing_git, code) in [
        (true, "board::mount::repo_missing"),
        (false, "board::mount::commit_missing"),
    ] {
        let board = Changes::new();
        board.mount_x(&[("README.md", "x\n")]);
        let gone = board.repository("repos/gone", None);
        board.building_in(&gone, WIP_LIMIT, "elsewhere");
        break_mount(&board, "repos/gone", losing_git);
        let (output, warnings) = board.warned(&[
            "changes.start",
            "--mount",
            "/repos/x",
            "--task",
            TASK,
            "--title",
            "Quick",
            "--flow",
            "small",
        ]);
        assert_eq!(output["phase"], "build");
        assert!(warnings.contains("/repos/gone"), "{warnings}");
        assert!(warnings.contains(code), "{warnings}");
        assert!(
            warnings.contains("`board mount update") || warnings.contains("change `repo`"),
            "{warnings}"
        );
    }
}

#[test]
fn the_limit_still_refuses_with_an_unreadable_mount_beside_a_full_repository() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    let gone = board.repository("repos/gone", None);
    board.building_in(&gone, 1, "elsewhere");
    break_mount(&board, "repos/gone", true);
    board.building_in("/repos/x", WIP_LIMIT, "busy");
    let input = json!({"mount": "/repos/x", "task": TASK, "title": "Quick", "flow": "small"});
    assert_eq!(board.refusal("start", &input)["code"], "changes::wip::full");
}

#[test]
fn a_change_for_the_mount_that_cannot_be_read_is_refused_with_the_mounts_code() {
    let board = Changes::new();
    board.repository("repos/gone", None);
    break_mount(&board, "repos/gone", true);
    let input = json!({"mount": "/repos/gone", "task": TASK, "title": "Quick", "flow": "small"});
    let refused = board.refusal("start", &input);
    assert_eq!(
        refused["fields"]["code"], "board::mount::repo_missing",
        "{refused}"
    );
}

#[test]
fn another_changes_mount_that_is_a_dangling_link_warns_and_start_carries_on() {
    let board = Changes::new();
    board.mount_x(&[("README.md", "x\n")]);
    board.write(
        "repos/dangling.json",
        r#"{"$type": "board/mount/v1", "path": "/nowhere"}"#,
    );
    board.building_in("/repos/dangling", WIP_LIMIT, "elsewhere");
    let (output, warnings) = board.warned(&[
        "changes.start",
        "--mount",
        "/repos/x",
        "--task",
        TASK,
        "--title",
        "Quick",
        "--flow",
        "small",
    ]);
    assert_eq!(output["phase"], "build");
    assert!(warnings.contains("/repos/dangling"), "{warnings}");
    assert!(warnings.contains("board::ref::dangling"), "{warnings}");
}

impl Changes {
    /// `board --json call <arguments>`: the output, and what it printed on
    /// stderr.
    fn warned(&self, arguments: &[&str]) -> (Value, String) {
        let mut all = vec!["--json", "call"];
        all.extend_from_slice(arguments);
        let output = self.run(&all, "");
        let warnings = text(&output.stderr);
        assert_eq!(output.code, 0, "{warnings}");
        (json(&output)["data"]["output"].clone(), warnings)
    }
}
