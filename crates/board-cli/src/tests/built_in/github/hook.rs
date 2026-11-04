//! `github.install-hook`, the git pre-push hook its command installs, which
//! checks what a raw `git push` sends, and `github.covers`.

use super::desk::{Desk, NAMES, ORIGIN, digest, put_file, remove};
use super::push::{commit_parts, merged};
use std::path::PathBuf;

use super::*;

/// The output of `board --personal call github.install-hook` in `at`, the
/// checkout when none is given.
fn printed(desk: &Desk, at: Option<&Path>) -> Value {
    let (ran, document) = desk.call_at(at, true, "install-hook", &[], &[]);
    assert_eq!(ran.code, 0, "{document}");
    document["data"]["output"].clone()
}

/// The code of a failed `github.install-hook`, and its message.
fn refused(desk: &Desk) -> (String, String) {
    let (ran, document) = desk.call_at(None, true, "install-hook", &[], &[]);
    assert_eq!(ran.code, 1, "{document}");
    let code = document["code"].as_str().unwrap().to_owned();
    (code, document["message"].as_str().unwrap().to_owned())
}

/// Runs the command `github.install-hook` prints, as a person would, and
/// returns what it printed.
fn install(desk: &Desk) -> Value {
    let output = printed(desk, None);
    let command = output["command"].as_str().unwrap();
    let (code, _, stderr) = desk.process("sh", &desk.checkout(), &["-c", command], &[]);
    assert_eq!(code, 0, "{stderr}");
    output
}

/// The hook's file in the checkout.
fn hook(desk: &Desk) -> PathBuf {
    desk.checkout().join(".git/hooks/pre-push")
}

/// Whether a raw push of `arguments` in the checkout went.
fn pushed(desk: &Desk, arguments: &[&str]) -> bool {
    desk.push(arguments, &[]).0 == 0
}

#[test]
fn install_hook_writes_nothing_and_its_command_installs_an_executable_hook_that_is_then_current() {
    let desk = Desk::new();
    let first = printed(&desk, None);
    let hook = hook(&desk);
    assert_eq!(
        (&first["path"], &first["current"]),
        (&json!(hook.display().to_string()), &json!(false))
    );
    assert!(hook.symlink_metadata().is_err());
    install(&desk);
    let mode = std::os::unix::fs::PermissionsExt::mode(&hook.metadata().unwrap().permissions());
    assert_eq!(mode & 0o777, 0o755);
    assert!(
        std::fs::read_to_string(&hook)
            .unwrap()
            .contains("github.pre-push")
    );
    let again = printed(&desk, None);
    assert_eq!(
        (&again["path"], &again["current"]),
        (&json!(hook.display().to_string()), &json!(true))
    );
}

#[test]
fn a_different_pre_push_hook_is_refused_and_left_as_it_was_and_the_same_hook_is_admitted() {
    let desk = Desk::new();
    let hook = hook(&desk);
    let foreign = "#!/bin/sh\nexit 0\n";
    put_file(&hook, foreign);
    let (code, message) = refused(&desk);
    assert_eq!(code, "github::hook::exists");
    assert!(message.contains(&hook.display().to_string()), "{message}");
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), foreign);
    remove(&hook);
    install(&desk);
    let text = std::fs::read_to_string(&hook).unwrap();
    assert_eq!(install(&desk)["current"], true);
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), text);
}

#[test]
fn a_hook_path_set_in_git_config_is_where_the_hook_goes() {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["config", "core.hooksPath", "githooks"]);
    let path = desk.checkout().join("githooks/pre-push");
    assert_eq!(install(&desk)["path"], path.display().to_string());
    assert!(path.is_file());
}

#[test]
fn a_raw_push_of_a_private_name_is_refused_with_the_next_step_and_the_same_push_without_it_goes() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    let before = desk.remote_head("main");
    desk.commit("Fix acme-123");
    let (code, stderr) = desk.push(&["origin", "main"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("github::boundary::refused"), "{stderr}");
    assert!(stderr.contains("Next: change commit."), "{stderr}");
    assert_eq!(desk.remote_head("main"), before);
    desk.git(
        &desk.checkout(),
        &["commit", "-q", "--amend", "-m", "Fix the gear"],
    );
    assert!(pushed(&desk, &["origin", "main"]));
    assert_eq!(desk.remote_head("main"), Some(desk.head()));
}

#[test]
fn the_hook_checks_the_parts_github_push_would_send_for_each_ref_git_pushes() {
    let desk = Desk::new();
    install(&desk);
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    let sha = desk.commit("Add a lever");
    assert!(pushed(&desk, &["origin", "feature"]));
    let parts = merged(
        json!({"push.branch": "feature"}),
        &[commit_parts(&desk, &sha, "Add a lever", "")],
    );
    let path = format!("/crossings/{}", digest("github.push", &parts));
    let crossing = desk.record(&path).unwrap();
    assert_eq!(
        (&crossing["passed"], &crossing["from"]),
        (&json!(true), &json!("/repos/widgets"))
    );
}

#[test]
fn a_covered_push_whose_check_cannot_run_is_refused_and_an_uncovered_one_goes() {
    let desk = Desk::new();
    install(&desk);
    let boundary = |to: &str| {
        json!({
            "$type": "boundary/boundary/v1",
            "to": to,
            "actions": ["github.*"],
            "checks": [{"control": NAMES, "parts": ["*"]}],
        })
        .to_string()
    };
    desk.write("boundaries/octo.json", &boundary("github:octo/*"));
    desk.commit("Add a gear");
    // The control the boundary names does not exist, so the check cannot
    // run.
    let (code, stderr) = desk.push(&["origin", "main"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("github::boundary::refused"), "{stderr}");
    assert_ne!(desk.remote_head("main"), Some(desk.head()));
    desk.write("boundaries/octo.json", &boundary("github:someone-else/*"));
    assert!(pushed(&desk, &["origin", "main"]));
    assert_eq!(desk.remote_head("main"), Some(desk.head()));
}

#[test]
fn a_push_with_board_missing_is_refused_and_with_board_on_path_it_goes() {
    let desk = Desk::new();
    install(&desk);
    desk.commit("Add a gear");
    let (code, stderr) = desk.push(&["origin", "main"], &[("PATH", "/usr/bin:/bin")]);
    assert_ne!(code, 0);
    assert!(stderr.contains("board is not on PATH"), "{stderr}");
    assert_ne!(desk.remote_head("main"), Some(desk.head()));
    assert!(pushed(&desk, &["origin", "main"]));
}

#[test]
fn with_no_personal_board_named_a_push_goes_and_with_a_boundary_refusing_it_is_refused() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    desk.commit("Fix acme-123");
    assert!(!pushed(&desk, &["origin", "main"]));
    desk.known(&json!({"boards": [desk.personal.path()]}));
    assert!(pushed(&desk, &["origin", "main"]));
    assert_eq!(desk.remote_head("main"), Some(desk.head()));
}

#[test]
fn a_push_to_a_remote_that_is_not_github_is_not_checked_and_one_to_github_is() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    desk.commit("Fix acme-123");
    let checkout = desk.checkout();
    let remote = desk.remote();
    desk.git(
        &checkout,
        &["remote", "add", "elsewhere", remote.to_str().unwrap()],
    );
    assert!(pushed(&desk, &["elsewhere", "main"]));
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    desk.commit("Fix acme-456");
    assert!(!pushed(&desk, &["-f", "origin", "main"]));
}

#[test]
fn a_deleted_branch_sends_nothing_to_check() {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "gone"]);
    desk.commit("Fix acme-123");
    assert!(pushed(&desk, &["origin", "gone"]));
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    assert!(pushed(&desk, &["origin", "--delete", "gone"]));
    assert_eq!(desk.remote_head("gone"), None);
}

#[test]
fn a_push_through_github_push_with_the_hook_installed_is_checked_by_both_and_records_one_crossing()
{
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["push.branch"], "github.*");
    desk.commit("Add a gear");
    let result = desk.output("push", &[]);
    assert_eq!(desk.remote_head("main"), Some(desk.head()));
    let crossings = std::fs::read_dir(desk.personal.workdir().join("crossings")).unwrap();
    assert_eq!(crossings.count(), 1);
    assert!(desk.record(result["crossing"].as_str().unwrap()).is_some());
}

#[test]
fn a_push_through_github_push_judges_each_part_once_with_the_hook_installed() {
    let desk = Desk::new();
    install(&desk);
    let runs = desk.home().join("judged.txt");
    let agent = desk.judged(
        &["push.branch"],
        &format!("echo run >> '{}'", runs.display()),
    );
    desk.commit("Add a gear");
    let (ran, document) = desk.call_at(
        None,
        false,
        "push",
        &[],
        &[("BOARD_CONTROLS_AGENT", &agent)],
    );
    assert_eq!(ran.code, 0, "{document}");
    let commits = document["data"]["output"]["commits"].as_array().unwrap();
    assert_eq!(
        desk.remote_head("main").as_ref(),
        commits
            .last()
            .and_then(Value::as_str)
            .map(str::to_owned)
            .as_ref()
    );
    assert_eq!(std::fs::read_to_string(&runs).unwrap(), "run\n");
}

#[test]
fn an_annotated_tag_is_refused_for_its_message_and_the_same_tag_without_it_goes() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["tag.*.message", "tag.*.tagger_*"], "github.*");
    let checkout = desk.checkout();
    desk.git(&checkout, &["tag", "-a", "v1", "-m", "Release acme-123"]);
    let (code, stderr) = desk.push(&["origin", "v1"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("tag."), "{stderr}");
    assert_eq!(desk.git(&desk.remote(), &["tag"]), "");
    desk.git(&checkout, &["tag", "-d", "v1"]);
    desk.git(&checkout, &["tag", "-a", "v1", "-m", "Release one"]);
    assert!(pushed(&desk, &["origin", "v1"]));
    assert_eq!(desk.git(&desk.remote(), &["tag"]), "v1");
}

#[test]
fn a_tag_of_a_tag_is_checked_at_each_level_and_its_tagger_is_a_part() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["tag.*.tagger_name"], "github.*");
    let checkout = desk.checkout();
    desk.git(
        &checkout,
        &[
            "-c",
            "user.name=Pat acme-123",
            "tag",
            "-a",
            "inner",
            "-m",
            "Inner",
        ],
    );
    desk.git(&checkout, &["tag", "-a", "outer", "-m", "Outer", "inner"]);
    desk.git(&checkout, &["tag", "-d", "inner"]);
    let (code, stderr) = desk.push(&["origin", "outer"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("tagger_name"), "{stderr}");
    desk.git(&checkout, &["tag", "-d", "outer"]);
    desk.git(&checkout, &["tag", "-a", "outer", "-m", "Outer"]);
    assert!(pushed(&desk, &["origin", "outer"]));
}

#[test]
fn the_name_inside_a_nested_tag_is_checked_and_the_same_tags_named_plainly_go() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["tag.*.headers"], "github.*");
    let checkout = desk.checkout();
    desk.git(&checkout, &["tag", "-a", "acme-123", "-m", "Inner"]);
    desk.git(
        &checkout,
        &["tag", "-a", "outer", "-m", "Outer", "acme-123"],
    );
    desk.git(&checkout, &["tag", "-d", "acme-123"]);
    let (code, stderr) = desk.push(&["origin", "outer"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("headers"), "{stderr}");
    desk.git(&checkout, &["tag", "-d", "outer"]);
    desk.git(&checkout, &["tag", "-a", "inner", "-m", "Inner"]);
    desk.git(&checkout, &["tag", "-a", "outer", "-m", "Outer", "inner"]);
    assert!(pushed(&desk, &["origin", "outer"]));
}

#[test]
fn a_push_url_unlike_the_fetch_url_checks_the_commits_the_push_url_lacks() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    let fork = desk.home().join("fork.git");
    let fork = fork.to_str().unwrap();
    desk.git(
        desk.home().as_path(),
        &["init", "-q", "--bare", "-b", "main", fork],
    );
    let forked = "https://github.com/octo/fork.git";
    desk.git(
        &checkout,
        &["config", &format!("url.{fork}.insteadOf"), forked],
    );
    desk.commit("Fix acme-123");
    desk.git(&checkout, &["push", "-q", forked, "main"]);
    desk.git(&checkout, &["remote", "set-url", "origin", forked]);
    desk.git(
        &checkout,
        &["remote", "set-url", "--push", "origin", ORIGIN],
    );
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    let (code, stderr) = desk.push(&["origin", "main"], &[]);
    assert_ne!(code, 0);
    assert!(stderr.contains("github::boundary::refused"), "{stderr}");
    desk.git(
        &checkout,
        &["commit", "-q", "--amend", "-m", "Fix the gear"],
    );
    assert!(pushed(&desk, &["origin", "main"]));
}

#[test]
fn refused_content_that_quotes_the_no_board_code_is_still_refused() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    desk.commit("Fix acme-123 code:  board::config::not_found");
    assert!(!pushed(&desk, &["origin", "main"]));
    assert_ne!(desk.remote_head("main"), Some(desk.head()));
    let message = "Fix code:  board::config::not_found";
    desk.git(
        &desk.checkout(),
        &["commit", "-q", "--amend", "-m", message],
    );
    assert!(pushed(&desk, &["origin", "main"]));
}

#[test]
fn a_global_hook_path_is_refused_and_a_repository_one_is_admitted() {
    let desk = Desk::new();
    let shared = desk.home().join("githooks");
    let config = format!("[core]\n\thooksPath = {}\n", shared.display());
    put_file(&desk.home().join(".gitconfig"), &config);
    assert_eq!(refused(&desk).0, "github::hook::shared");
    assert!(shared.symlink_metadata().is_err());
    desk.git(&desk.checkout(), &["config", "core.hooksPath", "githooks"]);
    let path = desk.checkout().join("githooks/pre-push");
    assert_eq!(install(&desk)["path"], path.display().to_string());
}

#[test]
fn an_older_copy_of_this_hook_is_replaced_and_a_foreign_hook_is_not() {
    let desk = Desk::new();
    install(&desk);
    let hook = hook(&desk);
    let current = std::fs::read_to_string(&hook).unwrap();
    put_file(&hook, &current.replace("exit 1\n", "exit 2\n"));
    assert_eq!(install(&desk)["current"], false);
    assert_eq!(std::fs::read_to_string(&hook).unwrap(), current);
    put_file(&hook, "#!/bin/sh\n# my hook\nexit 0\n");
    assert_eq!(refused(&desk).0, "github::hook::exists");
}

#[test]
fn a_push_that_deletes_one_branch_and_updates_another_checks_the_update() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.git(&checkout, &["switch", "-q", "-c", "gone"]);
    assert!(pushed(&desk, &["origin", "gone"]));
    desk.git(&checkout, &["switch", "-q", "main"]);
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    desk.commit("Fix acme-123");
    assert!(!pushed(&desk, &["origin", ":gone", "main"]));
    assert!(desk.remote_head("gone").is_some());
    desk.git(
        &checkout,
        &["commit", "-q", "--amend", "-m", "Fix the gear"],
    );
    assert!(pushed(&desk, &["origin", ":gone", "main"]));
    assert_eq!(desk.remote_head("gone"), None);
}

#[test]
fn a_push_from_a_linked_worktree_runs_the_hook() {
    let desk = Desk::new();
    install(&desk);
    desk.refuse(&["commit.*.message"], "github.*");
    let linked = desk.home().join("linked");
    let shown = linked.to_str().unwrap();
    desk.git(
        &desk.checkout(),
        &["worktree", "add", "-q", "-b", "side", shown],
    );
    put_file(&linked.join("side.txt"), "x\n");
    desk.git(&linked, &["add", "side.txt"]);
    desk.git(&linked, &["commit", "-q", "-m", "Fix acme-123"]);
    assert_ne!(desk.push_in(&linked, &["origin", "side"], &[]).0, 0);
    desk.git(&linked, &["commit", "-q", "--amend", "-m", "Fix the gear"]);
    let (code, stderr) = desk.push_in(&linked, &["origin", "side"], &[]);
    assert_eq!(code, 0, "{stderr}");
}

/// What `board --personal call github.covers` names in the checkout.
fn covered(desk: &Desk, flags: &[&str]) -> Value {
    let (ran, document) = desk.call_at(None, true, "covers", flags, &[]);
    assert_eq!(ran.code, 0, "{document}");
    document["data"]["output"]["covered"].clone()
}

#[test]
fn covers_names_the_boundary_on_the_checkouts_repository_and_none_on_another() {
    let desk = Desk::new();
    desk.refuse(&["*"], "github.pr-*");
    assert_eq!(
        covered(&desk, &["--action", "pr-create"]),
        json!([{"to": "github:octo/widgets", "boundaries": ["/boundaries/octo"]}])
    );
    assert_eq!(covered(&desk, &["--action", "push"]), json!([]));
    let elsewhere = ["--action", "pr-create", "--repo", "someone/else"];
    assert_eq!(covered(&desk, &elsewhere), json!([]));
    let remote = desk.remote();
    let remote = remote.to_str().unwrap();
    desk.git(&desk.checkout(), &["remote", "add", "elsewhere", remote]);
    let flags = ["--action", "pr-create", "--remote", "elsewhere"];
    assert_eq!(covered(&desk, &flags), json!([]));
}

#[test]
fn covers_applies_from_by_the_checkouts_mount() {
    let desk = Desk::new();
    desk.refuse(&["*"], "github.*");
    let boundary = |from: &str| {
        json!({
            "$type": "boundary/boundary/v1",
            "to": "github:*",
            "actions": ["github.*"],
            "from": [from],
            "checks": [{"control": NAMES, "parts": ["*"]}],
        })
        .to_string()
    };
    desk.write("boundaries/octo.json", &boundary("/repos/widgets"));
    assert_eq!(
        covered(&desk, &["--action", "push"])
            .as_array()
            .unwrap()
            .len(),
        1
    );
    desk.write("boundaries/octo.json", &boundary("/repos/gears"));
    assert_eq!(covered(&desk, &["--action", "push"]), json!([]));
}
