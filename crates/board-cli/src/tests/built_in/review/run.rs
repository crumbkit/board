//! `review.run` reviews a brief's range with the stand-in agent and records
//! a report beside the brief. The Python tests of the agent's deadline, its
//! signals and its escaped children are board-plugin's child tests and
//! board-ops's agent tests, which start every process a built-in runs.

use std::path::PathBuf;

use super::*;

/// The patch id the plan defines: `git diff-tree -p --no-renames
/// --no-ext-diff --no-textconv --full-index <commit> | git patch-id
/// --verbatim`, or none for a commit with no diff.
#[expect(
    clippy::disallowed_methods,
    reason = "the expected patch id comes from git's own pipeline, as a person runs it"
)]
fn plan_patch_id(root: &Path, commit: &str) -> Option<String> {
    let command = format!(
        "git diff-tree -p --no-renames --no-ext-diff --no-textconv --full-index {commit} | git patch-id --verbatim"
    );
    let ran = std::process::Command::new("sh")
        .args(["-c", &command])
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(ran.status.success(), "{}", text(&ran.stderr));
    text(&ran.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
}

const FINDING: &str =
    r#"{"severity": "high", "file": "src/sync.rs", "line": 3, "text": "The retry never stops."}"#;

/// Two commits on top of the fixture's: the base and head of the range
/// under review.
fn changed(repo: &Repo) -> (String, String) {
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.commit("src/sync.rs", "fn sync() {}\n");
    let head = repo.commit("src/sync.rs", "fn sync() {\n    loop {}\n}\n");
    (base, head)
}

/// A brief at /reviews/fix-sync/brief of the range `changed` makes.
fn fix_sync(repo: &Repo) -> String {
    let (base, head) = changed(repo);
    repo.brief("fix-sync", &format!("{base}..{head}"), &json!({}))
}

/// The code of a refused run of `brief`, after checking it wrote no report.
fn failed(repo: &Repo, brief: &str, extra: &[(&str, &str)]) -> Value {
    let (code, document) = repo.call_with("run", &["--brief", brief], extra);
    assert_eq!(code, 1, "{document}");
    let report = format!(
        "{}report.json",
        brief.trim_end_matches("brief").trim_start_matches('/')
    );
    assert!(!repo.root().join(report).exists());
    document
}

#[test]
fn a_run_writes_the_report_beside_the_brief() {
    let repo = Repo::new();
    let (base, head) = changed(&repo);
    let brief = repo.brief("fix-sync", &format!("{base}..{head}"), &json!({}));
    let low = json!({"severity": "low", "text": "Name the loop."});
    let finding: Value = serde_json::from_str(FINDING).unwrap();
    repo.answers(&answer("changes", &json!([finding, low])), 0);
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    assert_eq!(
        output,
        json!({"report": "/reviews/fix-sync/report", "range": format!("{base}..{head}"), "verdict": "changes", "findings": 2})
    );
    let report = repo.get("/reviews/fix-sync/report");
    let commits = repo.git(&["rev-list", "--reverse", &format!("{base}..{head}")]);
    let expected: Vec<String> = (commits.split_whitespace())
        .filter_map(|commit| plan_patch_id(repo.root(), commit))
        .collect();
    assert_eq!(report["patch_ids"], json!(expected));
    assert_eq!(report["brief"], brief);
    assert_eq!(report["findings"], json!([finding, low]));
    assert_eq!(
        (&report["verdict"], &report["by"]),
        (&json!("changes"), &json!({"program": "sh"}))
    );
    assert!(report["cost"]["seconds"].as_f64().unwrap() >= 0.0);
    assert_eq!(report["cost"].get("input_tokens"), None);
    let mut written: Vec<String> = std::fs::read_dir(repo.root().join("reviews/fix-sync"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    written.sort();
    assert_eq!(written, ["brief.json", "report.json"]);
    assert_eq!(repo.git(&["status", "--porcelain"]), "");
    assert_eq!(repo.run(&["check"], &[]).code, 0);
}

#[test]
fn patch_ids_are_the_ones_the_plans_per_commit_command_gives() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.commit("src/a.txt", "one\n  indented\n");
    repo.commit("src/a.txt", "one\n\tindented\n");
    repo.commit("src/b.bin", "\0binary\0");
    repo.git(&["mv", "src/a.txt", "src/moved.txt"]);
    repo.git(&["commit", "-qm", "move"]);
    repo.git(&["rm", "-q", "src/b.bin"]);
    repo.git(&["commit", "-qm", "delete"]);
    repo.git(&["commit", "-q", "--allow-empty", "-m", "nothing"]);
    let head = repo.git(&["rev-parse", "HEAD"]);
    let commits = repo.git(&["rev-list", "--reverse", &format!("{base}..{head}")]);
    let expected: Vec<String> = (commits.split_whitespace())
        .filter_map(|commit| plan_patch_id(repo.root(), commit))
        .collect();
    assert_eq!(expected.len(), commits.split_whitespace().count() - 1);
    let brief = repo.brief("ids", &format!("{base}..{head}"), &json!({}));
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
    assert_eq!(
        repo.get("/reviews/ids/report")["patch_ids"],
        json!(expected)
    );
}

#[test]
fn two_binary_changes_to_one_path_get_different_patch_ids_and_one_change_on_two_bases_gets_one() {
    let repo = Repo::new();
    let base = repo.git(&["rev-parse", "HEAD"]);
    let ids = |name: &str, from: &str, to: &str| {
        let brief = repo.brief(name, &format!("{from}..{to}"), &json!({}));
        assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
        repo.get(&format!("/reviews/{name}/report"))["patch_ids"].clone()
    };
    let first = repo.commit("src/image.bin", "\0one\0");
    let first_ids = ids("first", &base, &first);
    repo.git(&["reset", "-q", "--hard", &base]);
    let second = repo.commit("src/image.bin", "\0two\0");
    let second_ids = ids("second", &base, &second);
    repo.git(&["reset", "-q", "--hard", &base]);
    let other = repo.commit("src/other.txt", "other\n");
    let rebased = repo.commit("src/image.bin", "\0one\0");
    let rebased_ids = ids("rebased", &other, &rebased);
    assert_ne!(first_ids, second_ids);
    assert_eq!(first_ids, rebased_ids);
}

#[test]
fn the_agent_reads_the_focus_questions_and_diff_and_a_briefs_follows_passes_to_the_report() {
    let repo = Repo::new();
    let (base, head) = changed(&repo);
    let range = format!("{base}..{head}");
    let (_, first) = repo.call("run", &["--brief", &repo.brief("one", &range, &json!({}))]);
    let fields = json!({"questions": ["Does it stop?"], "follows": first["report"]});
    let brief = repo.brief("two", &range, &fields);
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    let prompt = repo.prompt();
    for expected in [
        "Focus: Check the sync change.",
        "- Does it stop?",
        "+    loop {}",
    ] {
        assert!(prompt.contains(expected), "{expected}: {prompt}");
    }
    assert_eq!(repo.get("/reviews/two/report")["follows"], first["report"]);
}

#[test]
fn a_second_run_of_a_reviewed_brief_is_refused_and_a_brief_that_follows_it_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let (_, first) = repo.call("run", &["--brief", &brief]);
    let (code, document) = repo.call("run", &["--brief", &brief]);
    assert_eq!(
        (code, &document["code"], &document["fields"]["report"]),
        (1, &json!("review::report::exists"), &first["report"])
    );
    let range = repo.get("/reviews/fix-sync/brief")["range"]
        .as_str()
        .unwrap()
        .to_owned();
    let follows = json!({"follows": first["report"]});
    let (code, second) = repo.call(
        "run",
        &["--brief", &repo.brief("fix-sync-2", &range, &follows)],
    );
    assert_eq!(
        (code, &second["report"]),
        (0, &json!("/reviews/fix-sync-2/report")),
        "{second}"
    );
}

#[test]
fn claudes_json_envelope_gives_the_answer_and_tokens_and_the_reviewer_is_the_program() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let envelope = json!({
        "type": "result",
        "result": approving(),
        "usage": {
            "input_tokens": 10,
            "cache_creation_input_tokens": 5,
            "cache_read_input_tokens": 100,
            "output_tokens": 40,
        },
        "modelUsage": {"claude-test-model": {}},
    });
    repo.answers(&envelope.to_string(), 0);
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    let report = repo.get("/reviews/fix-sync/report");
    assert_eq!(report["by"], json!({"program": "sh"}));
    assert_eq!(
        (
            &report["cost"]["input_tokens"],
            &report["cost"]["output_tokens"]
        ),
        (&json!(115), &json!(40))
    );
}

#[test]
fn the_agent_sees_the_variables_the_manifest_names_and_not_others() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let extra = [("ANTHROPIC_API_KEY", "test-key"), ("OTHER_SECRET", "x")];
    let (code, output) = repo.call_with("run", &["--brief", &brief], &extra);
    assert_eq!(code, 0, "{output}");
    let seen = repo.agent_file("env.txt");
    assert!(
        seen.lines()
            .any(|line| line == "ANTHROPIC_API_KEY=test-key"),
        "{seen}"
    );
    assert!(
        !seen.lines().any(|line| line.starts_with("OTHER_SECRET=")),
        "{seen}"
    );
}

#[test]
fn git_does_not_see_the_model_credentials_that_the_agent_sees() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    // The stand-in appends its environment to `git-env.txt`.
    let git = stand_in_git(r#"env >> "$(dirname "$0")/git-env.txt""#);
    repo.bin.write("git", Item::Executable(&git));
    let extra = [
        ("ANTHROPIC_API_KEY", "test-key"),
        ("CLAUDE_CODE_OAUTH_TOKEN", "test-token"),
    ];
    let (code, output) = repo.call_with("run", &["--brief", &brief], &extra);
    assert_eq!(code, 0, "{output}");
    let agent = repo.agent_file("env.txt");
    let git = std::fs::read_to_string(repo.bin.workdir().join("git-env.txt")).unwrap();
    assert!(git.lines().any(|line| line.starts_with("PATH=")), "{git}");
    for (name, value) in extra {
        let line = format!("{name}={value}");
        assert!(agent.lines().any(|seen| seen == line), "{agent}");
        assert!(
            !git.lines()
                .any(|seen| seen.starts_with(&format!("{name}="))),
            "{git}"
        );
    }
}

#[test]
fn a_brief_that_is_not_there_is_refused_and_one_that_is_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let refused = failed(&repo, "/reviews/missing/brief", &[]);
    assert_eq!(refused["code"], "review::brief::not_found");
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn a_range_git_cannot_resolve_is_refused_and_one_it_can_runs() {
    let repo = Repo::new();
    let (base, head) = changed(&repo);
    let bad = repo.brief("bad", &format!("{}..{head}", "0".repeat(40)), &json!({}));
    assert_eq!(failed(&repo, &bad, &[])["code"], "review::range::unknown");
    let good = repo.brief("good", &format!("{base}..{head}"), &json!({}));
    assert_eq!(repo.call("run", &["--brief", &good]).0, 0);
}

#[test]
fn a_range_of_names_or_options_is_invalid_and_one_of_full_commit_ids_runs() {
    let repo = Repo::new();
    let (base, head) = changed(&repo);
    let written = repo.root().join("written");
    let ranges = [
        "main..HEAD".to_owned(),
        format!("{base}..--output={}", written.display()),
        format!("{}..{head}", &base[..12]),
    ];
    for range in &ranges {
        let brief = repo.brief("named", range, &json!({}));
        assert_eq!(
            failed(&repo, &brief, &[])["code"],
            "review::brief::not_found",
            "{range}"
        );
    }
    assert!(!written.exists());
    let good = repo.brief("good", &format!("{base}..{head}"), &json!({}));
    assert_eq!(repo.call("run", &["--brief", &good]).0, 0);
}

#[test]
fn an_agent_that_fails_is_refused_and_one_that_succeeds_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    repo.answers(&approving(), 3);
    let refused = failed(&repo, &brief, &[]);
    assert_eq!(
        (&refused["code"], &refused["fields"]["exit"]),
        (&json!("review::agent::failed"), &json!(3))
    );
    repo.answers(&approving(), 0);
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn an_agent_that_fails_with_its_error_on_stdout_carries_that_error_in_the_message() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    repo.answers(
        r#"{"type": "result", "is_error": true, "result": "Credit balance is too low"}"#,
        1,
    );
    let refused = failed(&repo, &brief, &[]);
    assert_eq!(refused["code"], "review::agent::failed");
    let message = refused["message"].as_str().unwrap();
    assert!(message.contains("Credit balance is too low"), "{message}");
}

#[test]
fn an_answer_without_a_valid_verdict_and_findings_is_refused_and_a_valid_one_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    for stdout in [
        "No JSON here.".to_owned(),
        answer("maybe", &json!([])),
        answer("approve", &json!([{"severity": "urgent", "text": "x"}])),
        answer("approve", &json!([{"severity": "low", "text": ""}])),
    ] {
        repo.answers(&stdout, 0);
        assert_eq!(
            failed(&repo, &brief, &[])["code"],
            "review::agent::bad_output",
            "{stdout}"
        );
    }
    repo.answers(
        &answer("approve", &json!([{"severity": "low", "text": "x"}])),
        0,
    );
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn an_agent_that_cannot_start_is_refused_and_one_that_can_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let missing = json!([repo.agent.workdir().join("no-such-agent")]).to_string();
    let refused = failed(&repo, &brief, &[("BOARD_REVIEW_AGENT", &missing)]);
    assert_eq!(refused["code"], "review::agent::not_started");
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn an_agent_variable_unset_or_not_an_array_of_strings_is_refused_and_one_that_is_runs() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let unset = failed(&repo, &brief, &[("BOARD_REVIEW_AGENT", "")]);
    assert_eq!(unset["code"], "review::agent::not_set");
    for value in ["sh agent.sh", "[]", r#"["", "agent.sh"]"#, r#""sh""#, "[1]"] {
        let refused = failed(&repo, &brief, &[("BOARD_REVIEW_AGENT", value)]);
        assert_eq!(refused["code"], "review::agent::invalid", "{value}");
    }
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn the_readmes_agent_with_an_empty_argument_runs_and_one_with_an_empty_program_is_refused() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let readme = include_str!("../../../../../../plugins/review/README.md");
    let line = (readme.lines())
        .find_map(|line| line.strip_prefix("export BOARD_REVIEW_AGENT='"))
        .expect("README.md shows BOARD_REVIEW_AGENT");
    let example: Vec<String> = serde_json::from_str(line.trim_end_matches('\'')).unwrap();
    assert_eq!(example[0], "claude");
    assert!(example[1..].contains(&String::new()));
    // A `claude` on PATH that records its argv, one argument a line, and
    // runs the stand-in agent.
    let argv = repo.agent.workdir().join("argv.txt");
    let claude = format!(
        "#!/bin/sh\nprintf \"%s\\n\" \"$@\" > \"{}\"\nexec sh \"{}\"\n",
        argv.display(),
        repo.agent_path()
    );
    repo.bin.write("claude", Item::Executable(&claude));
    let mut empty = example.clone();
    empty[0] = String::new();
    let empty = serde_json::to_string(&empty).unwrap();
    let refused = failed(&repo, &brief, &[("BOARD_REVIEW_AGENT", &empty)]);
    assert_eq!(refused["code"], "review::agent::invalid");
    let named = serde_json::to_string(&example).unwrap();
    let (code, output) = repo.call_with(
        "run",
        &["--brief", &brief],
        &[("BOARD_REVIEW_AGENT", &named)],
    );
    assert_eq!(code, 0, "{output}");
    let recorded = std::fs::read_to_string(argv).unwrap();
    let recorded: Vec<&str> = recorded.split('\n').collect();
    assert_eq!(recorded[..recorded.len() - 1], example[1..]);
}

#[test]
fn an_agent_named_in_the_boards_tree_is_never_started() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    let marker = repo.root().join("started");
    let command = json!({"command": ["touch", marker]}).to_string();
    repo.commit("reviews/agent.json", &command);
    assert_eq!(
        failed(&repo, &brief, &[("BOARD_REVIEW_AGENT", "")])["code"],
        "review::agent::not_set"
    );
    assert!(!marker.exists());
    assert_eq!(repo.call("run", &["--brief", &brief]).0, 0);
}

#[test]
fn a_brief_reached_through_a_mount_is_refused_and_one_in_the_boards_own_tree_runs() {
    let repo = Repo::new();
    let (base, head) = changed(&repo);
    let brief = json!({"$type": "review/brief/v1", "range": format!("{base}..{head}"), "focus": "Check it."})
        .to_string();
    let (_library, mount) = library(&[("brief.json", &brief)]);
    repo.commit("reviews/shared.json", &mount.to_string());
    assert_eq!(repo.run(&["get", "/reviews/shared/brief"], &[]).code, 0);
    assert_eq!(
        failed(&repo, "/reviews/shared/brief", &[])["code"],
        "review::brief::not_found"
    );
    repo.commit("reviews/own/brief.json", &brief);
    assert_eq!(repo.call("run", &["--brief", "/reviews/own/brief"]).0, 0);
}

#[test]
fn a_base_that_moved_after_the_branch_point_is_replaced_by_the_merge_base() {
    let repo = Repo::new();
    let point = repo.git(&["rev-parse", "HEAD"]);
    repo.git(&["checkout", "-q", "-b", "change"]);
    let head = repo.commit("src/sync.rs", "fn sync() {}\n");
    repo.git(&["checkout", "-q", "main"]);
    let moved = repo.commit("src/later.rs", "fn later() {}\n");
    repo.git(&["checkout", "-q", "change"]);
    let brief = repo.brief("fix-sync", &format!("{moved}..{head}"), &json!({}));
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    assert_eq!(output["range"], format!("{point}..{head}"));
    let prompt = repo.prompt();
    assert!(!prompt.contains("later"), "{prompt}");
    assert!(prompt.contains("+fn sync() {}"), "{prompt}");
}

#[test]
fn the_agent_starts_in_an_empty_directory_away_from_the_boards_mcp_servers() {
    let repo = Repo::new();
    let brief = fix_sync(&repo);
    repo.write(
        ".mcp.json",
        r#"{"mcpServers": {"tasks": {"command": "tasks-server"}}}"#,
    );
    let (code, output) = repo.call("run", &["--brief", &brief]);
    assert_eq!(code, 0, "{output}");
    let listed = repo.agent_file("directory.txt");
    let mut lines = listed.lines();
    let directory = PathBuf::from(lines.next().unwrap());
    assert_eq!(lines.collect::<Vec<_>>(), Vec::<&str>::new());
    let root = repo.root().canonicalize().unwrap();
    assert!(!directory.starts_with(&root), "{}", directory.display());
    assert!(!directory.exists());
}
