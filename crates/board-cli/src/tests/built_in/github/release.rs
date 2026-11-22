//! `tag` and `release-create`: a new tag and a published release reach
//! GitHub only after the boundary passes what they send, a tag is never
//! moved, and a release is published only once every file is uploaded.
//!
//! The stub gh answers each run by the first rule whose pattern matches its
//! arguments, and keeps each run's arguments and stdin, so a test sees
//! every call in order.
#![expect(
    clippy::disallowed_methods,
    reason = "the tests write their stub's rules and asset files outside any board"
)]

use super::desk::{Desk, digest, executable, put_bytes, put_file};
use super::*;

/// The stub gh. Each run appends its arguments, joined by spaces, as one
/// line of `gh-log.txt` and keeps its stdin in `gh-stdin-<n>.txt` and its
/// working directory in `gh-pwd-<n>.txt`. Then the
/// first rule in `rules/` whose `pattern`, a shell glob, matches the
/// arguments prints its `out` and `err` and exits with its `exit`.
const GH: &str = r#"#!/bin/sh
here="$(dirname "$0")"
printf '%s\n' "$*" >> "$here/gh-log.txt"
n=$(wc -l < "$here/gh-log.txt" | tr -d ' ')
cat > "$here/gh-stdin-$n.txt"
pwd -P > "$here/gh-pwd-$n.txt"
for rule in "$here"/rules/*; do
  [ -e "$rule/pattern" ] || continue
  pattern=$(cat "$rule/pattern")
  case "$*" in
  $pattern) cat "$rule/out"; cat "$rule/err" >&2; exit "$(cat "$rule/exit")";;
  esac
done
echo "no rule answers gh $*" >&2
exit 97
"#;

/// A git that keeps each run's arguments in `git-log.txt`, then runs git.
const GIT: &str = r#"#!/bin/sh
printf '%s\n' "$*" >> "$(dirname "$0")/git-log.txt"
exec /usr/bin/git "$@"
"#;

const TAG: &str = "v1.0.0";
const TAG_URL: &str = "https://github.com/octo/widgets/releases/tag/v1.0.0";
/// The tag object GitHub makes.
const OBJECT: &str = "0123456789abcdef0123456789abcdef01234567";
const NOT_FOUND: &str = "gh: Not Found (HTTP 404)";

/// A desk whose stub gh answers by rule, and whose checkout names its
/// tagger.
fn desk() -> Desk {
    let desk = Desk::new();
    put_file(&desk.bin().join("gh"), GH);
    let checkout = desk.checkout();
    desk.git(&checkout, &["config", "user.name", "Ada"]);
    desk.git(&checkout, &["config", "user.email", "ada@example.com"]);
    desk
}

/// Adds a rule: arguments matching `pattern` print `out` and `err` and
/// exit with `code`.
fn answer(desk: &Desk, pattern: &str, code: u8, out: &str, err: &str) {
    let rules = desk.bin().join("rules");
    let count = std::fs::read_dir(&rules).map_or(0, Iterator::count);
    let rule = rules.join(format!("{count:03}"));
    put_file(&rule.join("pattern"), pattern);
    put_file(&rule.join("out"), out);
    put_file(&rule.join("err"), err);
    put_file(&rule.join("exit"), &code.to_string());
}

/// Forgets every rule and every run.
fn reset(desk: &Desk) {
    let bin = desk.bin();
    let _ = std::fs::remove_dir_all(bin.join("rules"));
    let _ = std::fs::remove_file(bin.join("gh-log.txt"));
}

/// Each run of the stub gh, its arguments joined by spaces.
fn log(desk: &Desk) -> Vec<String> {
    std::fs::read_to_string(desk.bin().join("gh-log.txt"))
        .map(|log| log.lines().map(str::to_owned).collect())
        .unwrap_or_default()
}

/// The stdin of the stub gh's run numbered `n`, from 1.
fn stdin(desk: &Desk, n: usize) -> String {
    desk.gh_file(&format!("gh-stdin-{n}.txt"))
}

/// The ref read, default branch and comparison of a tag that GitHub lacks,
/// at a target on the default branch, then the two writes.
fn taggable(desk: &Desk) {
    answer(
        desk,
        "api repos/octo/widgets/git/ref/tags/*",
        1,
        "",
        NOT_FOUND,
    );
    answer(
        desk,
        "api repos/octo/widgets",
        0,
        r#"{"default_branch": "main"}"#,
        "",
    );
    answer(
        desk,
        "api repos/octo/widgets/compare/main...*",
        0,
        r#"{"status": "behind"}"#,
        "",
    );
    let object = json!({"sha": OBJECT, "tag": TAG}).to_string();
    answer(
        desk,
        "api --method POST repos/octo/widgets/git/tags --input -",
        0,
        &object,
        "",
    );
    let made = json!({"ref": "refs/tags/v1.0.0", "object": {"sha": OBJECT}}).to_string();
    answer(
        desk,
        "api --method POST repos/octo/widgets/git/refs --input -",
        0,
        &made,
        "",
    );
}

/// The flags of a tag of the checkout's head with `message`.
fn tag_flags<'a>(target: &'a str, message: &'a str) -> [&'a str; 6] {
    ["--name", TAG, "--target", target, "--message", message]
}

/// The calls a tag makes before it writes.
fn reads(target: &str) -> Vec<String> {
    vec![
        "api repos/octo/widgets/git/ref/tags/v1.0.0".to_owned(),
        "api repos/octo/widgets".to_owned(),
        format!("api repos/octo/widgets/compare/main...{target}"),
    ]
}

/// The working directory of the stub gh's run numbered `n`, from 1.
fn pwd(desk: &Desk, n: usize) -> std::path::PathBuf {
    desk.gh_file(&format!("gh-pwd-{n}.txt")).trim().into()
}

/// The code of a call that failed.
fn code_of((ran, document): (Output, Value)) -> Value {
    assert_eq!(ran.code, 1, "{document}");
    document["code"].clone()
}

/// Whether the stub gh was asked to write anything.
fn wrote(desk: &Desk) -> bool {
    (log(desk).iter())
        .any(|line| line.contains("--method POST") || line.starts_with("release create"))
}

#[test]
fn tag_posts_the_tag_object_and_the_ref_with_the_checked_parts_and_records_the_crossing() {
    let desk = desk();
    taggable(&desk);
    let target = desk.head();
    let result = desk.output("tag", &tag_flags(&target, "Widgets 1.0"));
    let mut expected = reads(&target);
    expected.extend([
        "api --method POST repos/octo/widgets/git/tags --input -".to_owned(),
        "api --method POST repos/octo/widgets/git/refs --input -".to_owned(),
    ]);
    assert_eq!(log(&desk), expected);
    let object: Value = serde_json::from_str(&stdin(&desk, 4)).unwrap();
    assert_eq!(
        object,
        json!({
            "tag": TAG,
            "message": "Widgets 1.0",
            "object": target,
            "type": "commit",
            "tagger": {"name": "Ada", "email": "ada@example.com"},
        })
    );
    let reference: Value = serde_json::from_str(&stdin(&desk, 5)).unwrap();
    assert_eq!(reference, json!({"ref": "refs/tags/v1.0.0", "sha": OBJECT}));
    let parts = json!({
        "tag.message": "Widgets 1.0",
        "tag.name": TAG,
        "tag.tagger_email": "ada@example.com",
        "tag.tagger_name": "Ada",
        "tag.target": target,
    });
    assert_eq!(result["digest"], json!(digest("github.tag", &parts)));
    assert_eq!(
        (
            &result["sent"],
            &result["name"],
            &result["target"],
            &result["object"]
        ),
        (&json!(TAG_URL), &json!(TAG), &json!(target), &json!(OBJECT))
    );
    let note = result["note"].as_str().unwrap();
    assert!(note.contains("git fetch origin tag v1.0.0"), "{note}");
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&crossing["action"], &crossing["result"]),
        (&json!("github.tag"), &json!(TAG_URL))
    );
}

#[test]
fn a_tag_github_already_has_is_refused_before_any_write_and_a_new_one_is_made() {
    let desk = desk();
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    let existing = json!({"ref": "refs/tags/v1.0.0", "object": {"sha": OBJECT}}).to_string();
    answer(
        &desk,
        "api repos/octo/widgets/git/ref/tags/*",
        0,
        &existing,
        "",
    );
    assert_eq!(code_of(desk.call("tag", &flags)), "github::tag::exists");
    assert_eq!(log(&desk), ["api repos/octo/widgets/git/ref/tags/v1.0.0"]);
    reset(&desk);
    taggable(&desk);
    assert_eq!(desk.output("tag", &flags)["object"], OBJECT);
}

#[test]
fn a_ref_made_after_the_read_is_refused_as_existing_and_never_moved_and_a_free_one_is_made() {
    let desk = desk();
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    answer(
        &desk,
        "api --method POST repos/octo/widgets/git/refs --input -",
        1,
        "",
        "gh: Reference already exists (HTTP 422)",
    );
    taggable(&desk);
    assert_eq!(code_of(desk.call("tag", &flags)), "github::tag::exists");
    let patched = log(&desk).iter().any(|line| line.contains("PATCH"));
    assert!(!patched, "a tag is never moved");
    reset(&desk);
    taggable(&desk);
    assert_eq!(desk.output("tag", &flags)["sent"], TAG_URL);
}

#[test]
fn githubs_other_refusals_of_the_tag_object_or_the_ref_carry_its_message_and_not_exists() {
    let desk = desk();
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    let refusals = [
        (
            "git/tags",
            "gh: Validation Failed: tagger.email is invalid (HTTP 422)",
        ),
        ("git/refs", "gh: Reference update failed (HTTP 422)"),
    ];
    for (endpoint, said) in refusals {
        reset(&desk);
        let pattern = format!("api --method POST repos/octo/widgets/{endpoint} --input -");
        answer(&desk, &pattern, 1, "", said);
        taggable(&desk);
        let (ran, document) = desk.call("tag", &flags);
        assert_eq!(
            code_of((ran, document.clone())),
            "github::gh::failed",
            "{endpoint}"
        );
        let message = document["message"].as_str().unwrap();
        let github = said.trim_start_matches("gh: ");
        assert!(message.contains(github), "{endpoint}: {message}");
    }
    reset(&desk);
    taggable(&desk);
    assert_eq!(desk.output("tag", &flags)["object"], OBJECT);
}

#[test]
fn a_target_not_on_the_default_branch_is_refused_and_one_identical_to_it_is_tagged() {
    let desk = desk();
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    for status in ["ahead", "diverged"] {
        reset(&desk);
        let compared = json!({"status": status}).to_string();
        answer(
            &desk,
            "api repos/octo/widgets/compare/main...*",
            0,
            &compared,
            "",
        );
        taggable(&desk);
        assert_eq!(
            code_of(desk.call("tag", &flags)),
            "github::tag::target_not_merged",
            "{status}"
        );
        assert!(!wrote(&desk));
    }
    reset(&desk);
    let identical = json!({"status": "identical"}).to_string();
    answer(
        &desk,
        "api repos/octo/widgets/compare/main...*",
        0,
        &identical,
        "",
    );
    taggable(&desk);
    assert_eq!(desk.output("tag", &flags)["target"], target);
}

#[test]
fn a_target_github_does_not_have_is_refused_and_one_it_has_is_tagged() {
    let desk = desk();
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    answer(
        &desk,
        "api repos/octo/widgets/compare/main...*",
        1,
        "",
        NOT_FOUND,
    );
    taggable(&desk);
    assert_eq!(
        code_of(desk.call("tag", &flags)),
        "github::tag::target_not_found"
    );
    assert!(!wrote(&desk));
    reset(&desk);
    taggable(&desk);
    assert_eq!(desk.output("tag", &flags)["target"], target);
}

#[test]
fn an_abbreviated_target_or_an_invalid_name_is_refused_by_board_and_their_twins_are_tagged() {
    let desk = desk();
    taggable(&desk);
    let target = desk.head();
    let short = &target[..12];
    let refused = |name: &str, target: &str| {
        let flags = ["--name", name, "--target", target, "--message", "Widgets"];
        assert_eq!(
            code_of(desk.call("tag", &flags)),
            "board::plugin::bad_input",
            "{name} {target}"
        );
    };
    refused(TAG, short);
    let long = format!("v{}", "1".repeat(100));
    for name in [
        ".v1",
        "_v1",
        "v1..0",
        "v1.",
        "v1.lock",
        "v 1",
        "v1/0",
        long.as_str(),
    ] {
        refused(name, &target);
    }
    assert_eq!(log(&desk), Vec::<String>::new());
    let longest = format!("v{}", "1".repeat(99));
    for name in [
        TAG,
        "v1.0.0-rc.1+build.2",
        "v1.0",
        "release_7.lockdown",
        longest.as_str(),
    ] {
        let flags = ["--name", name, "--target", &target, "--message", "Widgets"];
        let (ran, document) = desk.call("tag", &flags);
        assert_eq!(ran.code, 0, "{name}: {document}");
    }
}

#[test]
fn a_checkout_with_no_tagger_email_is_refused_before_gh_runs_and_one_with_it_is_tagged() {
    let desk = desk();
    taggable(&desk);
    let target = desk.head();
    let flags = tag_flags(&target, "Widgets 1.0");
    desk.git(&desk.checkout(), &["config", "--unset", "user.email"]);
    let (ran, document) = desk.call("tag", &flags);
    assert_eq!(code_of((ran, document.clone())), "github::tag::no_tagger");
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("git config user.email"), "{message}");
    assert_eq!(log(&desk), Vec::<String>::new());
    desk.git(
        &desk.checkout(),
        &["config", "user.email", "ada@example.com"],
    );
    assert_eq!(desk.output("tag", &flags)["name"], TAG);
}

#[test]
fn a_boundary_refusing_the_tag_message_stops_the_tag_before_any_write_and_a_clean_one_is_sent() {
    let desk = desk();
    desk.refuse(&["tag.message"], "github.tag");
    taggable(&desk);
    let target = desk.head();
    let (ran, document) = desk.call("tag", &tag_flags(&target, "For acme-123"));
    assert_eq!(
        code_of((ran, document.clone())),
        "github::boundary::refused"
    );
    assert_eq!(document["fields"]["code"], "boundary::check::failed");
    assert_eq!(log(&desk), reads(&target));
    assert!(!wrote(&desk));
    reset(&desk);
    taggable(&desk);
    let result = desk.output("tag", &tag_flags(&target, "Widgets 1.0"));
    assert_eq!(result["sent"], TAG_URL);
}

#[test]
fn tag_runs_no_git_that_walks_history_and_runs_as_many_git_commands_after_more_commits() {
    let desk = desk();
    let git = desk.bin().join("git");
    put_file(&git, GIT);
    executable(&git);
    let counted = |target: &str| {
        let _ = std::fs::remove_file(desk.bin().join("git-log.txt"));
        reset(&desk);
        taggable(&desk);
        desk.output("tag", &tag_flags(target, "Widgets 1.0"));
        let log = std::fs::read_to_string(desk.bin().join("git-log.txt")).unwrap();
        let walking = (log.lines()).filter(|line| {
            [" rev-list", " ls-remote", " push", " log"]
                .iter()
                .any(|command| line.contains(command))
        });
        assert_eq!(walking.count(), 0, "{log}");
        log.lines().count()
    };
    let first = counted(&desk.head());
    for n in 0..20 {
        desk.commit(&format!("Change {n}"));
    }
    assert_eq!(counted(&desk.head()), first);
}

/// The ref read, release lookup, draft, uploads and publish of a release
/// of a tag GitHub has and no release has yet.
fn releasable(desk: &Desk) {
    let existing = json!({"ref": "refs/tags/v1.0.0", "object": {"sha": OBJECT}}).to_string();
    answer(
        desk,
        "api repos/octo/widgets/git/ref/tags/*",
        0,
        &existing,
        "",
    );
    answer(desk, "release view *", 1, "", "release not found\n");
    let draft = "https://github.com/octo/widgets/releases/tag/untagged-1a2b\n";
    answer(desk, "release create *", 0, draft, "");
    answer(desk, "release upload *", 0, "", "");
    answer(desk, "release edit *", 0, &format!("{TAG_URL}\n"), "");
}

/// Writes the files a release publishes in `dist/` in the checkout.
fn dist(desk: &Desk) -> std::path::PathBuf {
    let dist = desk.checkout().join("dist");
    put_bytes(&dist.join("widgets.dmg"), &[0, 159, 146, 150]);
    put_file(&dist.join("SHA256SUMS"), "abc  widgets.dmg\n");
    dist
}

const RELEASE_FLAGS: [&str; 8] = [
    "--tag",
    TAG,
    "--title",
    "Widgets 1.0",
    "--notes",
    "It turns.",
    "--assets",
    "dist",
];

/// The run of `gh release <verb>` for `tag` in the stub's log.
fn release(verb: &str, rest: &str) -> String {
    format!("release {verb} {TAG} --repo octo/widgets{rest}")
}

/// The run of `gh release upload` of the file `name`.
fn upload(name: &str) -> String {
    release("upload", &format!(" {name} --clobber"))
}

/// The run of `gh release edit` that publishes the draft titled `title`.
fn publish(title: &str) -> String {
    release(
        "edit",
        &format!(" --draft=false --title {title} --notes-file -"),
    )
}

#[test]
fn release_create_drafts_uploads_each_file_then_publishes_and_records_the_crossing() {
    let desk = desk();
    releasable(&desk);
    let dist = dist(&desk);
    let result = desk.output("release-create", &RELEASE_FLAGS);
    let view = " --json isDraft,assets,url";
    let create = " --verify-tag --draft --title Widgets 1.0 --notes-file -";
    assert_eq!(
        log(&desk),
        [
            "api repos/octo/widgets/git/ref/tags/v1.0.0".to_owned(),
            release("view", view),
            release("create", create),
            upload("SHA256SUMS"),
            upload("widgets.dmg"),
            publish("Widgets 1.0"),
        ]
    );
    assert_eq!(stdin(&desk, 3), "It turns.");
    assert_eq!(stdin(&desk, 6), "It turns.");
    let dist = dist.canonicalize().unwrap();
    assert_eq!((pwd(&desk, 4), pwd(&desk, 5)), (dist.clone(), dist));
    let parts = json!({
        "release.assets": "SHA256SUMS\nwidgets.dmg",
        "release.notes": "It turns.",
        "release.tag": TAG,
        "release.title": "Widgets 1.0",
    });
    assert_eq!(
        result["digest"],
        json!(digest("github.release-create", &parts))
    );
    assert_eq!(
        (
            &result["sent"],
            &result["tag"],
            &result["assets"],
            &result["resumed"]
        ),
        (
            &json!(TAG_URL),
            &json!(TAG),
            &json!(["SHA256SUMS", "widgets.dmg"]),
            &json!(false)
        )
    );
    let crossing = desk.record(result["crossing"].as_str().unwrap()).unwrap();
    assert_eq!(
        (&crossing["action"], &crossing["result"]),
        (&json!("github.release-create"), &json!(TAG_URL))
    );
}

#[test]
fn a_release_with_no_assets_and_no_title_is_titled_by_its_tag_and_published() {
    let desk = desk();
    releasable(&desk);
    let result = desk.output("release-create", &["--tag", TAG, "--notes", "It turns."]);
    assert_eq!(result["assets"], json!([]));
    let created = (log(&desk).into_iter()).find(|line| line.starts_with("release create"));
    let title = " --verify-tag --draft --title v1.0.0 --notes-file -";
    assert_eq!(created, Some(release("create", title)));
    assert!(
        !log(&desk)
            .iter()
            .any(|line| line.starts_with("release upload"))
    );
}

#[test]
fn a_release_of_a_tag_github_lacks_is_refused_before_any_write_and_one_it_has_is_published() {
    let desk = desk();
    dist(&desk);
    answer(
        &desk,
        "api repos/octo/widgets/git/ref/tags/*",
        1,
        "",
        NOT_FOUND,
    );
    releasable(&desk);
    assert_eq!(
        code_of(desk.call("release-create", &RELEASE_FLAGS)),
        "github::release::no_tag"
    );
    assert_eq!(log(&desk), ["api repos/octo/widgets/git/ref/tags/v1.0.0"]);
    reset(&desk);
    releasable(&desk);
    assert_eq!(
        desk.output("release-create", &RELEASE_FLAGS)["sent"],
        TAG_URL
    );
}

#[test]
fn a_tag_with_a_published_release_is_refused_and_one_with_none_is_published() {
    let desk = desk();
    dist(&desk);
    let published = json!({"isDraft": false, "url": TAG_URL, "assets": []}).to_string();
    answer(&desk, "release view *", 0, &published, "");
    releasable(&desk);
    assert_eq!(
        code_of(desk.call("release-create", &RELEASE_FLAGS)),
        "github::release::exists"
    );
    assert!(!wrote(&desk));
    assert!(
        !log(&desk)
            .iter()
            .any(|line| line.starts_with("release upload"))
    );
    reset(&desk);
    releasable(&desk);
    assert_eq!(
        desk.output("release-create", &RELEASE_FLAGS)["resumed"],
        false
    );
}

#[test]
fn an_asset_directory_holding_a_directory_a_link_a_hidden_file_or_a_spaced_name_is_refused() {
    let desk = desk();
    releasable(&desk);
    let dist = dist(&desk);
    let refused = |what: &str| {
        let (ran, document) = desk.call("release-create", &RELEASE_FLAGS);
        assert_eq!(
            code_of((ran, document.clone())),
            "github::release::asset_refused",
            "{what}"
        );
        let message = document["message"].as_str().unwrap();
        assert!(message.contains(what), "{message}");
        assert_eq!(log(&desk), Vec::<String>::new(), "{what}");
    };
    std::fs::create_dir(dist.join("nested")).unwrap();
    refused("nested");
    std::fs::remove_dir(dist.join("nested")).unwrap();
    std::os::unix::fs::symlink(dist.join("SHA256SUMS"), dist.join("sums")).unwrap();
    refused("sums");
    std::fs::remove_file(dist.join("sums")).unwrap();
    put_file(&dist.join(".DS_Store"), "x");
    refused(".DS_Store");
    std::fs::remove_file(dist.join(".DS_Store")).unwrap();
    put_file(&dist.join("read me.txt"), "x");
    refused("read me.txt");
    std::fs::remove_file(dist.join("read me.txt")).unwrap();
    let (ran, document) = desk.call(
        "release-create",
        &["--tag", TAG, "--notes", "x", "--assets", "missing"],
    );
    assert_eq!(code_of((ran, document)), "github::release::asset_refused");
    let result = desk.output("release-create", &RELEASE_FLAGS);
    assert_eq!(result["assets"], json!(["SHA256SUMS", "widgets.dmg"]));
}

#[test]
fn more_than_a_hundred_assets_are_refused_and_a_hundred_are_published() {
    let desk = desk();
    releasable(&desk);
    let dist = desk.checkout().join("dist");
    for n in 0..101 {
        put_file(&dist.join(format!("part-{n:03}.txt")), "x");
    }
    let (ran, document) = desk.call("release-create", &RELEASE_FLAGS);
    assert_eq!(
        code_of((ran, document.clone())),
        "github::release::asset_refused"
    );
    let message = document["message"].as_str().unwrap();
    assert!(message.contains("101"), "{message}");
    std::fs::remove_file(dist.join("part-100.txt")).unwrap();
    let result = desk.output("release-create", &RELEASE_FLAGS);
    assert_eq!(result["assets"].as_array().unwrap().len(), 100);
}

#[test]
fn a_rerun_after_a_failed_upload_resumes_the_draft_skips_the_file_uploaded_and_publishes() {
    let desk = desk();
    let dist = dist(&desk);
    answer(
        &desk,
        "release upload * widgets.dmg *",
        1,
        "",
        "upload timed out",
    );
    releasable(&desk);
    assert_eq!(
        code_of(desk.call("release-create", &RELEASE_FLAGS)),
        "github::gh::failed"
    );
    assert!(
        !log(&desk)
            .iter()
            .any(|line| line.starts_with("release edit"))
    );
    reset(&desk);
    let sums = std::fs::metadata(dist.join("SHA256SUMS")).unwrap().len();
    let draft = json!({
        "isDraft": true,
        "url": "https://github.com/octo/widgets/releases/tag/untagged-1a2b",
        "assets": [
            {"name": "SHA256SUMS", "size": sums, "state": "uploaded"},
            {"name": "widgets.dmg", "size": 4, "state": "starter"},
        ],
    });
    answer(&desk, "release view *", 0, &draft.to_string(), "");
    releasable(&desk);
    let result = desk.output("release-create", &RELEASE_FLAGS);
    assert_eq!(
        log(&desk)[1..],
        [
            release("view", " --json isDraft,assets,url"),
            upload("widgets.dmg"),
            publish("Widgets 1.0"),
        ]
    );
    assert_eq!(
        (&result["resumed"], &result["sent"]),
        (&json!(true), &json!(TAG_URL))
    );
}

#[test]
fn a_draft_file_of_another_size_is_uploaded_again() {
    let desk = desk();
    dist(&desk);
    let draft = json!({
        "isDraft": true,
        "url": "https://github.com/octo/widgets/releases/tag/untagged-1a2b",
        "assets": [
            {"name": "SHA256SUMS", "size": 1, "state": "uploaded"},
            {"name": "widgets.dmg", "size": 4, "state": "uploaded"},
        ],
    });
    answer(&desk, "release view *", 0, &draft.to_string(), "");
    releasable(&desk);
    desk.output("release-create", &RELEASE_FLAGS);
    let uploads: Vec<String> = (log(&desk).into_iter())
        .filter(|line| line.starts_with("release upload"))
        .collect();
    assert_eq!(uploads, [upload("SHA256SUMS")]);
}

#[test]
fn a_draft_holding_a_file_the_assets_lack_is_refused_and_one_without_it_is_published_as_checked() {
    let desk = desk();
    dist(&desk);
    // A draft made elsewhere, with its own notes and a file Board never saw.
    let draft = |assets: Value| {
        json!({
            "isDraft": true,
            "name": "Old title",
            "body": "Notes nobody checked.",
            "url": "https://github.com/octo/widgets/releases/tag/untagged-1a2b",
            "assets": assets,
        })
        .to_string()
    };
    let extra = json!([{"name": "acme-notes.txt", "size": 9, "state": "uploaded"}]);
    answer(&desk, "release view *", 0, &draft(extra), "");
    releasable(&desk);
    let (ran, document) = desk.call("release-create", &RELEASE_FLAGS);
    assert_eq!(
        code_of((ran, document.clone())),
        "github::release::stray_asset"
    );
    assert_eq!(document["fields"]["asset"], "acme-notes.txt");
    let wrote = (log(&desk).iter()).any(|line| {
        ["release upload", "release edit"]
            .iter()
            .any(|verb| line.starts_with(verb))
    });
    assert!(!wrote, "{:?}", log(&desk));
    reset(&desk);
    answer(&desk, "release view *", 0, &draft(json!([])), "");
    releasable(&desk);
    let result = desk.output("release-create", &RELEASE_FLAGS);
    assert_eq!(result["resumed"], true);
    let ran = log(&desk);
    assert_eq!(ran.last(), Some(&publish("Widgets 1.0")));
    assert_eq!(stdin(&desk, ran.len()), "It turns.");
}

#[test]
fn an_asset_directory_whose_path_holds_a_hash_uploads_each_file_by_its_bare_name() {
    let desk = desk();
    releasable(&desk);
    let dist = desk.checkout().join("dist#2");
    put_file(&dist.join("widgets.dmg"), "x");
    let flags = ["--tag", TAG, "--notes", "It turns.", "--assets", "dist#2"];
    let result = desk.output("release-create", &flags);
    assert_eq!(result["assets"], json!(["widgets.dmg"]));
    let ran = log(&desk);
    let at = ran
        .iter()
        .position(|line| line.starts_with("release upload"));
    assert_eq!(ran[at.unwrap()], upload("widgets.dmg"));
    assert_eq!(pwd(&desk, at.unwrap() + 1), dist.canonicalize().unwrap());
}

#[test]
fn a_boundary_refusing_an_asset_name_stops_the_release_before_the_draft_and_a_clean_one_is_sent() {
    let desk = desk();
    desk.refuse(&["release.assets"], "github.release-create");
    releasable(&desk);
    let dist = dist(&desk);
    put_file(&dist.join("acme-123.txt"), "x");
    let (ran, document) = desk.call("release-create", &RELEASE_FLAGS);
    assert_eq!(
        code_of((ran, document.clone())),
        "github::boundary::refused"
    );
    assert!(!wrote(&desk));
    std::fs::remove_file(dist.join("acme-123.txt")).unwrap();
    assert_eq!(
        desk.output("release-create", &RELEASE_FLAGS)["sent"],
        TAG_URL
    );
}

#[test]
fn covers_names_the_boundary_on_tag_and_release_create_and_none_on_push() {
    let desk = desk();
    desk.refuse(&["*"], "github.tag");
    let covered = |action: &str| {
        let (ran, document) = desk.call_at(None, true, "covers", &["--action", action], &[]);
        assert_eq!(ran.code, 0, "{document}");
        document["data"]["output"]["covered"].clone()
    };
    let octo = json!([{"to": "github:octo/widgets", "boundaries": ["/boundaries/octo"]}]);
    assert_eq!(covered("tag"), octo);
    assert_eq!(covered("push"), json!([]));
    desk.refuse(&["*"], "github.release-create");
    assert_eq!(covered("release-create"), octo);
    assert_eq!(covered("tag"), json!([]));
}
