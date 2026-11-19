//! `merge` moves the mount of the merged repository to the commit GitHub's
//! merge made, and when it cannot, it still merges and warns with the
//! command to run by hand.

use super::desk::Desk;
use super::merge::{CHANGE, HEAD, TESTED, change, merging, pulls};
use super::*;

const MOUNT: &str = "/repos/widgets";
const WARNING: &str = "github::merge::mount";

/// Makes a commit on the bare remote's `main` with the same tree, as a
/// squash merge on GitHub does, leaving the checkout as it was, and returns
/// the commit.
fn landed(desk: &Desk) -> String {
    let remote = desk.remote();
    let tree = desk.git(&remote, &["rev-parse", "main^{tree}"]);
    let parent = ["-p", "main", "-m", "Add a lever (#12)"];
    let arguments: Vec<&str> = ["commit-tree", &tree].into_iter().chain(parent).collect();
    let commit = desk.git(&remote, &arguments);
    desk.git(&remote, &["update-ref", "refs/heads/main", &commit]);
    commit
}

/// The commit the personal board pins /repos/widgets at.
fn pin(desk: &Desk) -> String {
    desk.record(MOUNT).unwrap()["commit"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// A desk with the change in land, a stub gh that merges as `merged` says,
/// and a commit landed on the remote, which it returns.
fn ready(merged: bool) -> (Desk, String) {
    let desk = Desk::new();
    change(&desk, "land", HEAD, TESTED);
    pulls(&desk, merged);
    desk.gh_answers(&json!(""), 0, "");
    let commit = landed(&desk);
    (desk, commit)
}

/// What `fetch` and `mount update` the warning tells the agent to run.
fn by_hand(desk: &Desk, personal: &str) -> String {
    format!(
        "git -C {} fetch origin main && board {personal}mount update {MOUNT}",
        desk.checkout().display()
    )
}

/// The output of a merge of the change that exits 0, and its stderr.
fn merged(desk: &Desk) -> (Value, String) {
    let (ran, document) = desk.call("merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 0, "{document}");
    (document["data"]["output"].clone(), text(&ran.stderr))
}

#[test]
fn a_merge_by_change_or_by_head_moves_the_mount_to_the_merged_commit() {
    let (desk, commit) = ready(true);
    let before = pin(&desk);
    let (result, _) = merged(&desk);
    assert_eq!(result["phase"], "done");
    assert_eq!(
        result["mount"],
        json!({"path": MOUNT, "old": before, "new": commit, "wrote": true})
    );
    assert_eq!(result.get("mount_warning"), None);
    assert_eq!(pin(&desk), commit);

    let (desk, commit) = ready(true);
    let result = desk.output(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--head", HEAD]),
    );
    assert_eq!(result["mount"]["new"], json!(commit), "{result}");
    assert_eq!(pin(&desk), commit);
}

#[test]
fn a_queued_merge_leaves_the_pin_and_warns_and_once_merged_moves_it() {
    let (desk, commit) = ready(false);
    let before = pin(&desk);
    let (result, stderr) = merged(&desk);
    assert_eq!(result["queued"], true);
    assert_eq!(result.get("mount"), None);
    let warning = &result["mount_warning"];
    assert_eq!(
        (&warning["code"], warning.get("cause"), &warning["command"]),
        (&json!(WARNING), None, &json!(by_hand(&desk, "")))
    );
    assert!(stderr.contains(WARNING), "{stderr}");
    assert_eq!(pin(&desk), before);
    pulls(&desk, true);
    let (result, _) = merged(&desk);
    assert_eq!(result["mount"]["new"], json!(commit), "{result}");
    assert_eq!(pin(&desk), commit);
}

#[test]
fn an_unreachable_remote_still_merges_and_warns_and_a_reachable_one_moves_the_pin() {
    for reachable in [false, true] {
        let (desk, commit) = ready(true);
        let before = pin(&desk);
        if !reachable {
            let checkout = desk.checkout();
            let remote = desk.remote().display().to_string();
            desk.git(
                &checkout,
                &["config", "--remove-section", &format!("url.{remote}")],
            );
            let gone = format!("url.{remote}.gone.insteadOf");
            desk.git(&checkout, &["config", &gone, super::desk::ORIGIN]);
        }
        let (result, _) = merged(&desk);
        assert_eq!(result["phase"], "done");
        if reachable {
            assert_eq!(result["mount"]["new"], json!(commit), "{result}");
            assert_eq!(result.get("mount_warning"), None);
            assert_eq!(pin(&desk), commit);
        } else {
            let warning = &result["mount_warning"];
            assert_eq!(
                (&warning["code"], &warning["cause"], &warning["command"]),
                (
                    &json!(WARNING),
                    &json!("github::git::failed"),
                    &json!(by_hand(&desk, ""))
                ),
            );
            assert_eq!(result.get("mount"), None);
            assert_eq!(pin(&desk), before);
        }
    }
}

#[test]
fn a_change_naming_a_missing_mount_still_reaches_done_and_warns_and_its_twin_moves_the_mount() {
    for mount in ["/repos/gone", MOUNT] {
        let (desk, commit) = ready(true);
        let before = pin(&desk);
        let mut record = desk.record(CHANGE).unwrap();
        record["mount"] = json!(mount);
        desk.write("changes/lever.json", &record.to_string());
        let (result, _) = merged(&desk);
        assert_eq!(result["phase"], "done", "{mount}");
        if mount == MOUNT {
            assert_eq!(result["mount"]["new"], json!(commit), "{result}");
            assert_eq!(pin(&desk), commit);
        } else {
            let warning = &result["mount_warning"];
            assert_eq!(
                (&warning["code"], &warning["cause"], &warning["command"]),
                (
                    &json!(WARNING),
                    &json!("board::path::not_found"),
                    &json!("board mount update /repos/gone")
                ),
            );
            assert_eq!(pin(&desk), before);
        }
    }
}

#[test]
fn a_mount_following_a_local_branch_is_left_and_warned_of_and_one_following_origin_moves() {
    for follow in ["main", "origin/main"] {
        let (desk, commit) = ready(true);
        let before = pin(&desk);
        let mut mount = desk.record(MOUNT).unwrap();
        mount["follow"] = json!(follow);
        desk.write("repos/widgets.json", &mount.to_string());
        let (result, _) = merged(&desk);
        if follow == "main" {
            let warning = &result["mount_warning"];
            assert_eq!(
                (&warning["code"], warning.get("cause"), &warning["command"]),
                (
                    &json!(WARNING),
                    None,
                    &json!(format!("board mount update {MOUNT}"))
                ),
            );
            assert_eq!(result.get("mount"), None);
            assert_eq!(pin(&desk), before);
        } else {
            assert_eq!(result["mount"]["new"], json!(commit), "{result}");
            assert_eq!(pin(&desk), commit);
        }
    }
}
