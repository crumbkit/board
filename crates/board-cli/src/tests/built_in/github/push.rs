//! `github.push`: the commits a push would send are checked at the
//! boundary, and pushed only on a pass.

use super::desk::{Desk, NAMES, ORIGIN, TO, digest, executable, put_bytes, put_file, remove};
use super::*;

/// The parts of one commit made as `t <t@t>`, named by its short SHA.
pub(super) fn commit_parts(desk: &Desk, sha: &str, message: &str, binary: &str) -> Value {
    let flags = [
        "-c",
        "core.quotePath=false",
        "show",
        "--format=",
        "--diff-merges=first-parent",
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--text",
        sha,
    ];
    let diff = desk.git(&desk.checkout(), &flags);
    let short = &sha[..12];
    json!({
        format!("commit.{short}.message"): format!("{message}\n"),
        format!("commit.{short}.author_name"): "t",
        format!("commit.{short}.author_email"): "t@t",
        format!("commit.{short}.committer_name"): "t",
        format!("commit.{short}.committer_email"): "t@t",
        format!("commit.{short}.diff"): format!("{diff}\n"),
        format!("commit.{short}.headers"): "",
        format!("commit.{short}.binary"): binary,
    })
}

/// `base` with the fields of each of `others` over it.
pub(super) fn merged(base: Value, others: &[Value]) -> Value {
    let mut base = base;
    for other in others {
        for (name, value) in other.as_object().unwrap() {
            base[name] = value.clone();
        }
    }
    base
}

/// The refusal of a failed call: its exit code and code.
fn refused(ran: &Output, document: &Value) -> (u8, String) {
    (
        ran.code,
        document["code"].as_str().unwrap_or_default().to_owned(),
    )
}

fn code(desk: &Desk, flags: &[&str]) -> (u8, String) {
    let (ran, document) = desk.call("push", flags);
    refused(&ran, &document)
}

#[test]
fn a_push_checks_each_new_commit_by_name_then_pushes_and_records_the_crossing() {
    let desk = Desk::new();
    let first = desk.commit("Add a gear");
    let second = desk.commit_to("Add a spring", "spring.txt", None);
    let result = desk.output("push", &[]);
    let parts = merged(
        json!({"push.branch": "main"}),
        &[
            commit_parts(&desk, &first, "Add a gear", ""),
            commit_parts(&desk, &second, "Add a spring", ""),
        ],
    );
    assert_eq!(result["digest"], digest("github.push", &parts));
    assert_eq!(
        (&result["branch"], &result["commits"]),
        (&json!("main"), &json!([first, second]))
    );
    assert_eq!(desk.remote_head("main"), Some(second.clone()));
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    // A crossing stores no `passed`; `boundary.record` sets `result` only on
    // one that passed.
    assert_eq!(
        (crossing.get("passed"), &crossing["to"], &crossing["from"]),
        (None, &json!(TO), &json!("/repos/widgets"))
    );
    assert_eq!(crossing["result"], result["sent"]);
    let sent = result["sent"].as_str().unwrap();
    assert!(
        sent.starts_with(&format!("{second}:refs/heads/main")),
        "{sent}"
    );
    let upstream = ["rev-parse", "--abbrev-ref", "main@{upstream}"];
    assert_eq!(desk.git(&desk.checkout(), &upstream), "origin/main");
}

#[test]
fn a_new_branch_sends_only_the_commits_its_remote_lacks() {
    let desk = Desk::new();
    desk.git(&desk.checkout(), &["switch", "-q", "-c", "feature"]);
    let only = desk.commit("Add a lever");
    let result = desk.output("push", &[]);
    assert_eq!(
        (&result["branch"], &result["commits"]),
        (&json!("feature"), &json!([only]))
    );
    assert_eq!(desk.remote_head("feature"), Some(only));
    let upstream = ["rev-parse", "--abbrev-ref", "feature@{upstream}"];
    assert_eq!(desk.git(&desk.checkout(), &upstream), "origin/feature");
}

#[test]
fn a_private_name_in_a_commit_message_is_refused_with_the_boundarys_next_step_and_nothing_is_pushed()
 {
    let desk = Desk::new();
    desk.refuse(&["commit.*.message"], "github.*");
    let before = desk.remote_head("main");
    let sha = desk.commit("Fix acme-123");
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::boundary::refused".to_owned())
    );
    let fields = &failure["fields"];
    assert_eq!(fields["code"], "boundary::check::failed");
    let hint = failure["recovery"]["text"].as_str().unwrap();
    assert!(hint.starts_with("Next: change commit."), "{hint}");
    let message = failure["message"].as_str().unwrap();
    assert!(message.ends_with(hint), "{message}");
    assert_eq!(
        fields["failures"],
        json!([{"rule": NAMES, "part": format!("commit.{}.message", &sha[..12]), "text": "acme-123", "line": 1}])
    );
    assert_eq!(desk.remote_head("main"), before);
    desk.git(
        &desk.checkout(),
        &["commit", "-q", "--amend", "-m", "Fix the gear"],
    );
    assert_eq!(desk.output("push", &[])["commits"], json!([desk.head()]));
    assert_ne!(desk.remote_head("main"), before);
}

#[test]
fn a_private_name_in_a_diff_an_author_email_or_a_name_is_refused_and_the_same_commit_without_it_is_pushed()
 {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.refuse(
        &[
            "commit.*.diff",
            "commit.*.author_*",
            "commit.*.committer_name",
        ],
        "github.*",
    );
    desk.commit_to("Add a gear", "widget.txt", Some("see acme-123\n"));
    let (_, failure) = desk.call("push", &[]);
    let part = failure["fields"]["failures"][0]["part"].as_str().unwrap();
    assert_eq!(part.rsplit('.').next(), Some("diff"), "{part}");
    for (setting, part) in [
        ("user.email=acme-456@example.com", ".author_email"),
        ("user.name=Pat (acme-789)", ".author_name"),
    ] {
        desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
        let commit = [
            "-c",
            setting,
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "Empty",
        ];
        desk.git(&checkout, &commit);
        let (_, failure) = desk.call("push", &[]);
        let parts: Vec<&str> = (failure["fields"]["failures"].as_array().unwrap().iter())
            .filter_map(|failure| failure["part"].as_str())
            .collect();
        assert!(
            parts.iter().any(|each| each.ends_with(part)),
            "{setting}: {parts:?}"
        );
    }
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    desk.commit_to("Add a gear", "widget.txt", Some("fine\n"));
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_checkout_the_personal_board_does_not_mount_is_checked_without_from_and_a_mounted_one_names_its_mount()
 {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.commit("Add a gear");
    desk.git(&checkout, &["switch", "-q", "-c", "mounted"]);
    let (_, document) = desk.call_at(None, true, "push", &[], &[]);
    let mounted = desk.record(document["data"]["output"]["crossing"].as_str().unwrap());
    assert_eq!(mounted.unwrap()["from"], "/repos/widgets");
    remove(&desk.personal.workdir().join("repos/widgets.json"));
    desk.git(&checkout, &["switch", "-q", "-c", "unmounted"]);
    let (_, document) = desk.call_at(None, true, "push", &[], &[]);
    let unmounted = desk.record(document["data"]["output"]["crossing"].as_str().unwrap());
    assert_eq!(unmounted.unwrap().get("from"), None);
}

#[test]
fn a_detached_head_is_refused_before_the_personal_board_is_read_and_the_same_commit_on_a_branch_is_pushed()
 {
    let desk = Desk::new();
    let sha = desk.commit("Add a gear");
    desk.git(&desk.checkout(), &["switch", "-q", "--detach", &sha]);
    desk.known(&json!({"boards": [desk.personal.path()]}));
    for flags in [&[][..], &["--title", "T", "--body", "B"]] {
        let capability = if flags.is_empty() {
            "push"
        } else {
            "pr-create"
        };
        let (ran, failure) = desk.call(capability, flags);
        assert_eq!(
            refused(&ran, &failure),
            (1, "github::checkout::detached".to_owned()),
            "{capability}"
        );
    }
    let personal = desk.personal.path();
    desk.known(&json!({"boards": [personal], "personal": personal}));
    desk.git(&desk.checkout(), &["switch", "-q", "main"]);
    assert_eq!(desk.output("push", &[])["commits"], json!([sha]));
}

#[test]
fn a_remote_that_is_not_github_is_refused_and_a_github_remote_is_checked() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.commit("Add a gear");
    let remote = desk.remote().display().to_string();
    desk.git(&checkout, &["remote", "set-url", "origin", &remote]);
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::remote::not_github".to_owned())
    );
    assert!(failure["message"].as_str().unwrap().contains(&remote));
    let scp = "git@github.com:Octo/Widgets.git";
    desk.git(&checkout, &["remote", "set-url", "origin", scp]);
    desk.git(
        &checkout,
        &["config", &format!("url.{remote}.insteadOf"), scp],
    );
    assert_eq!(desk.output("push", &[])["branch"], "main");
}

#[test]
fn a_push_git_refuses_is_reported_and_the_crossing_keeps_no_result() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.commit("Add a gear");
    desk.git(&checkout, &["push", "-q", "origin", "main"]);
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    desk.commit_to("Add a spring", "spring.txt", None);
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::push::failed".to_owned())
    );
    let digest = failure["fields"]["digest"].as_str().unwrap();
    let crossing = desk
        .record(&format!("/crossings/{}", &digest[7..]))
        .unwrap();
    assert_eq!(crossing.get("result"), None);
    desk.git(&checkout, &["pull", "-q", "--rebase", "origin", "main"]);
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_call_outside_a_git_repository_is_refused_and_one_inside_is_admitted() {
    let desk = Desk::new();
    let (ran, failure) = desk.call_at(Some(&desk.home()), true, "push", &[], &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::checkout::not_found".to_owned())
    );
    let (ran, document) = desk.call_at(None, true, "push", &[], &[]);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["branch"], "main");
}

#[test]
fn a_mount_in_a_board_other_than_the_personal_one_gives_no_from() {
    let desk = Desk::new();
    let work = Scenario::new();
    let config = r#"{"$type": "board/config/v1", "commit_on_put": false}"#;
    work.write(".board/board.json", Item::File(config));
    let mount = desk.personal.workdir().join("repos/widgets.json");
    let mounted = std::fs::read_to_string(&mount).unwrap();
    work.write("widgets.json", Item::File(&mounted));
    remove(&mount);
    let personal = desk.personal.path();
    desk.known(&json!({"boards": [work.path()], "personal": personal}));
    let enabled = desk.run_in(work.workdir(), &["plugin", "enable", "github"], &[]);
    assert_eq!(enabled.code, 0, "{}", text(&enabled.stderr));
    desk.commit("Add a gear");
    let crossing = desk.output("push", &[])["crossing"].clone();
    let crossing = desk.record(crossing.as_str().unwrap()).unwrap();
    assert_eq!(crossing.get("from"), None);
}

#[test]
fn text_hidden_from_diffs_by_an_attribute_is_checked_and_binary_paths_are_named() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.refuse(&["commit.*.diff"], "github.*");
    let hide = |desk: &Desk| {
        put_file(&checkout.join(".gitattributes"), "*.txt -diff\n");
        desk.git(&checkout, &["add", ".gitattributes"]);
    };
    hide(&desk);
    let sha = desk.commit_to("Add notes", "notes.txt", Some("token acme-123\n"));
    let (_, failure) = desk.call("push", &[]);
    let part = format!("commit.{}.diff", &sha[..12]);
    assert_eq!(failure["fields"]["failures"][0]["part"], part);
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    hide(&desk);
    let sha = desk.commit_to("Add notes", "notes.txt", Some("token fine\n"));
    let result = desk.output("push", &[]);
    let parts = merged(
        json!({"push.branch": "main"}),
        &[commit_parts(&desk, &sha, "Add notes", "notes.txt\n")],
    );
    assert_eq!(result["digest"], digest("github.push", &parts));
}

#[test]
fn the_push_sends_the_commit_checked_even_when_the_branch_moves_during_the_check() {
    let desk = Desk::new();
    let checkout = desk.checkout().display().to_string();
    let later = format!(
        "git -C '{checkout}' -c user.name=t -c user.email=t@t commit -q --allow-empty -m Later"
    );
    let agent = desk.judged(&["push.branch"], &later);
    let checked = desk.commit("Add a gear");
    let (ran, document) = desk.call_at(None, false, "push", &[], &[("BOARD_RULES_AGENT", &agent)]);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["commits"], json!([checked]));
    assert_eq!(desk.remote_head("main"), Some(checked));
    assert_eq!(
        desk.git(&desk.checkout(), &["log", "-1", "--format=%s"]),
        "Later"
    );
}

#[test]
fn a_push_url_unlike_the_fetch_url_is_refused_and_the_same_checkout_without_it_is_pushed() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.commit("Add a gear");
    let secret = "https://user:secret@github.com/me/widgets.git";
    desk.git(
        &checkout,
        &["remote", "set-url", "--push", "origin", secret],
    );
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::remote::split".to_owned())
    );
    assert!(!failure.to_string().contains("secret"));
    desk.git(&checkout, &["config", "--unset", "remote.origin.pushurl"]);
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn tags_git_would_follow_are_not_pushed_and_the_branch_is() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.git(&checkout, &["config", "push.followTags", "true"]);
    let sha = desk.commit("Add a gear");
    desk.git(&checkout, &["tag", "-a", "v1", "-m", "Release acme-123"]);
    desk.output("push", &[]);
    assert_eq!(desk.remote_head("main"), Some(sha));
    assert_eq!(desk.git(&desk.remote(), &["tag"]), "");
}

#[test]
fn an_origin_url_holding_a_password_is_never_quoted_and_a_plain_one_is() {
    let desk = Desk::new();
    let secret = "https://user:secret@gitlab.example/octo/widgets.git";
    desk.git(&desk.checkout(), &["remote", "set-url", "origin", secret]);
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::remote::not_github".to_owned())
    );
    assert!(!failure.to_string().contains("secret"));
    let message = failure["message"].as_str().unwrap();
    assert!(
        message.contains("https://gitlab.example/octo/widgets.git"),
        "{message}"
    );
}

#[test]
fn a_remote_git_cannot_reach_is_refused_and_a_reachable_one_is_pushed() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.commit("Add a gear");
    let missing = format!(
        "url.{}.insteadOf",
        desk.home().join("missing.git").display()
    );
    let reachable = format!("url.{}.insteadOf", desk.remote().display());
    desk.git(&checkout, &["config", "--replace-all", &missing, ORIGIN]);
    desk.git(&checkout, &["config", "--unset", &reachable]);
    assert_eq!(code(&desk, &[]), (1, "github::git::failed".to_owned()));
    desk.git(&checkout, &["config", "--unset", &missing]);
    desk.git(&checkout, &["config", &reachable, ORIGIN]);
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_personal_board_without_boundary_is_refused_and_with_it_enabled_the_push_goes() {
    let desk = Desk::new();
    desk.commit("Add a gear");
    let file = desk.personal.workdir().join(".board/board.json");
    let enabled = std::fs::read_to_string(&file).unwrap();
    let mut settings: Value = serde_json::from_str(&enabled).unwrap();
    let plugins = settings["plugins"].as_array_mut().unwrap();
    plugins.retain(|name| name != "boundary");
    desk.write(".board/board.json", &settings.to_string());
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::board::refused".to_owned())
    );
    assert_eq!(failure["fields"]["code"], "board::plugin::not_enabled");
    assert_ne!(desk.remote_head("main"), Some(desk.head()));
    desk.write(".board/board.json", &enabled);
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_single_branch_clone_pushes_a_new_branch_and_sets_its_upstream() {
    let desk = Desk::new();
    let clone = desk.home().join("clone");
    let remote = desk.remote().display().to_string();
    let arguments = [
        "clone",
        "-q",
        "--single-branch",
        "-b",
        "main",
        &remote,
        clone.to_str().unwrap(),
    ];
    desk.git(&desk.home(), &arguments);
    desk.git(&clone, &["remote", "set-url", "origin", ORIGIN]);
    desk.git(
        &clone,
        &["config", &format!("url.{remote}.insteadOf"), ORIGIN],
    );
    desk.git(&clone, &["switch", "-q", "-c", "feature"]);
    desk.git(
        &clone,
        &["commit", "-q", "--allow-empty", "-m", "Add a lever"],
    );
    desk.gh_answers(&json!("https://github.com/Octo/Widgets/pull/12\n"), 0, "");
    let flags = ["--title", "Add a lever", "--body", "Levers."];
    let (ran, document) = desk.call_at(Some(&clone), true, "pr-create", &flags, &[]);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(document["data"]["output"]["number"], 12);
    let head = desk.git(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(desk.remote_head("feature"), Some(head));
    assert_eq!(
        desk.git(&clone, &["config", "branch.feature.merge"]),
        "refs/heads/feature"
    );
}

#[test]
fn a_commit_of_a_file_git_lfs_stores_is_refused_and_the_same_file_outside_lfs_is_pushed() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    let pointer = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 12\n",
        "0".repeat(64)
    );
    for (attributes, stored) in [
        ("*.csv filter=lfs diff=lfs merge=lfs -text\n", true),
        ("*.csv text\n", false),
    ] {
        put_file(&checkout.join(".gitattributes"), attributes);
        desk.git(&checkout, &["add", ".gitattributes"]);
        let sha = desk.commit_to("Add data", "data.csv", Some(&pointer));
        let before = desk.remote_head("main");
        let (ran, result) = desk.call("push", &[]);
        if stored {
            assert_eq!(refused(&ran, &result), (1, "github::push::lfs".to_owned()));
            assert_eq!(result["fields"]["paths"], json!(["data.csv"]));
            assert_eq!(desk.remote_head("main"), before);
            desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
        } else {
            assert_eq!(ran.code, 0, "{result}");
            assert_eq!(result["data"]["output"]["commits"], json!([sha]));
        }
    }
}

#[test]
fn a_private_name_with_a_non_ascii_letter_in_a_file_name_is_refused_and_its_ascii_twin_is_pushed() {
    let desk = Desk::new();
    desk.refuse(&["commit.*.diff"], "github.*");
    desk.write("private/names.txt", "Müller\n");
    desk.commit_to("Add a list", "Müller.txt", Some("fine\n"));
    let (_, failure) = desk.call("push", &[]);
    assert_eq!(failure["fields"]["failures"][0]["text"], "Müller");
    desk.git(&desk.checkout(), &["reset", "-q", "--hard", "HEAD~1"]);
    desk.commit_to("Add a list", "Muller.txt", Some("fine\n"));
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn a_private_name_in_a_commit_header_is_refused_and_a_commit_without_it_is_pushed() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    desk.refuse(&["commit.*.headers"], "github.*");
    let encoded = [
        "-c",
        "i18n.commitEncoding=acme-123",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "Empty",
    ];
    desk.git(&checkout, &encoded);
    let (_, failure) = desk.call("push", &[]);
    let part = failure["fields"]["failures"][0]["part"].as_str().unwrap();
    assert!(part.ends_with(".headers"), "{part}");
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    desk.git(&checkout, &["commit", "-q", "--allow-empty", "-m", "Empty"]);
    assert_eq!(
        desk.output("push", &[])["commits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn git_hooks_never_see_the_model_credentials_or_gh_tokens_gh_sees_only_its_tokens_and_the_boundarys_agent_sees_the_credentials()
 {
    let desk = Desk::new();
    let hook = desk.checkout().join(".git/hooks/pre-push");
    let seen = desk.home().join("hook-env.txt");
    put_file(&hook, &format!("#!/bin/sh\nenv > '{}'\n", seen.display()));
    executable(&hook);
    let judged = desk.home().join("agent-env.txt");
    let agent = desk.judged(&["*"], &format!("env > '{}'", judged.display()));
    let keys = [
        ("ANTHROPIC_API_KEY", "model-key-1"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "model-token-1"),
        ("GH_TOKEN", "gh-token-1"),
        ("GITHUB_TOKEN", "github-token-1"),
        ("BOARD_RULES_AGENT", &agent),
    ];
    desk.commit("Add a gear");
    let (ran, document) = desk.call_at(None, false, "push", &[], &keys);
    assert_eq!(ran.code, 0, "{document}");
    let url = "https://github.com/Octo/Widgets/issues/7#issuecomment-1";
    desk.gh_answers(&json!({"html_url": url}), 0, "");
    let flags = ["--number", "7", "--body", "Fine."];
    let (ran, document) = desk.call_at(None, false, "comment", &flags, &keys);
    assert_eq!(ran.code, 0, "{document}");
    let hooked = std::fs::read_to_string(&seen).unwrap();
    let gh = desk.gh_file("gh-env.txt");
    let judging = std::fs::read_to_string(&judged).unwrap();
    for (_, value) in &keys[..2] {
        assert!(!hooked.contains(value), "the hook saw {value}");
        assert!(!gh.contains(value), "gh saw {value}");
        assert!(judging.contains(value), "the agent did not see {value}");
    }
    for (_, value) in &keys[2..4] {
        assert!(!hooked.contains(value), "the hook saw {value}");
        assert!(gh.contains(value), "gh did not see {value}");
    }
    assert!(hooked.contains("HOME="), "the hook saw no environment");
}

#[test]
fn a_text_file_that_is_not_utf8_is_refused_and_its_utf8_twin_and_a_binary_file_are_pushed() {
    let desk = Desk::new();
    let checkout = desk.checkout();
    let commit = |bytes: &[u8], file: &str| {
        put_bytes(&checkout.join(file), bytes);
        desk.git(&checkout, &["add", file]);
        desk.git(&checkout, &["commit", "-q", "-m", "Add names"]);
        desk.head()
    };
    // Latin-1, which git counts as text.
    let sha = commit(b"M\xfcller\n", "names.txt");
    let (ran, failure) = desk.call("push", &[]);
    assert_eq!(
        refused(&ran, &failure),
        (1, "github::git::not_utf8".to_owned())
    );
    assert_eq!(failure["fields"]["commit"], sha);
    assert_eq!(
        desk.remote_head("main"),
        Some(desk.git(&checkout, &["rev-parse", "HEAD~1"]))
    );
    desk.git(&checkout, &["reset", "-q", "--hard", "HEAD~1"]);
    let utf8 = commit("M\u{fc}ller\n".as_bytes(), "names.txt");
    assert_eq!(desk.output("push", &[])["commits"], json!([utf8]));
    let binary = commit(b"\x89PNG\x00\xff\xfe", "image.png");
    let pushed = desk.output("push", &[]);
    assert_eq!(pushed["commits"], json!([binary]));
}

#[test]
fn a_push_failing_on_authentication_while_a_gh_token_is_set_is_refused_with_a_sign_in_hint_and_without_the_failure_it_goes()
 {
    let desk = Desk::new();
    let hook = desk.checkout().join(".git/hooks/pre-push");
    // git's own words when HTTPS sign-in fails.
    let failing = "#!/bin/sh\necho \"fatal: Authentication failed for 'https://github.com/Octo/Widgets.git/'\" >&2\nexit 1\n";
    put_file(&hook, failing);
    executable(&hook);
    desk.commit("Add a gear");
    let hint = |token: &[(&str, &str)]| {
        let (ran, failure) = desk.call_at(None, false, "push", &[], token);
        assert_eq!(
            refused(&ran, &failure),
            (1, "github::push::failed".to_owned())
        );
        failure["recovery"]["text"].as_str().map(str::to_owned)
    };
    let signed = hint(&[("GH_TOKEN", "gh-token-1")]).unwrap();
    assert!(signed.contains("gh auth login"), "{signed}");
    assert!(signed.contains("SSH"), "{signed}");
    assert_eq!(
        hint(&[]).filter(|text| text.contains("gh auth login")),
        None
    );
    remove(&hook);
    let pushed = desk.call_at(None, false, "push", &[], &[("GH_TOKEN", "gh-token-1")]);
    assert_eq!(pushed.0.code, 0, "{}", pushed.1);
}

#[test]
fn a_remote_listing_that_fails_to_sign_in_while_a_gh_token_is_set_carries_the_sign_in_hint_and_a_reachable_remote_is_pushed()
 {
    let desk = Desk::new();
    let checkout = desk.checkout();
    // A remote helper on PATH that fails as git does when HTTPS sign-in
    // fails, which `ls-remote` meets before anything is checked.
    let helper = desk.bin().join("git-remote-unsigned");
    put_file(
        &helper,
        "#!/bin/sh\necho \"fatal: Authentication failed for 'https://github.com/Octo/Widgets.git/'\" >&2\nexit 128\n",
    );
    executable(&helper);
    let remote = desk.remote().display().to_string();
    let reachable = format!("url.{remote}.insteadOf");
    desk.git(&checkout, &["config", "--unset", &reachable]);
    desk.git(&checkout, &["config", "url.unsigned::x.insteadOf", ORIGIN]);
    desk.commit("Add a gear");
    let hint = |token: &[(&str, &str)]| {
        let (ran, failure) = desk.call_at(None, false, "push", &[], token);
        assert_eq!(
            refused(&ran, &failure),
            (1, "github::git::failed".to_owned())
        );
        failure["recovery"]["text"].as_str().map(str::to_owned)
    };
    let signed = hint(&[("GH_TOKEN", "gh-token-1")]).unwrap();
    assert!(signed.contains("gh auth login"), "{signed}");
    assert_eq!(
        hint(&[]).filter(|text| text.contains("gh auth login")),
        None
    );
    desk.git(
        &checkout,
        &["config", "--unset", "url.unsigned::x.insteadOf"],
    );
    desk.git(&checkout, &["config", &reachable, ORIGIN]);
    let pushed = desk.call_at(None, false, "push", &[], &[("GH_TOKEN", "gh-token-1")]);
    assert_eq!(pushed.0.code, 0, "{}", pushed.1);
}
