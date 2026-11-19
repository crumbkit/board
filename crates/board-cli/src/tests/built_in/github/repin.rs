//! `merge` moves the mount of the merged repository to the commit GitHub's
//! merge made, and when it cannot, it still merges and warns with the
//! command to run by hand.

use super::desk::Desk;
use super::merge::{CHANGE, HEAD, TEAM_TESTED, TESTED, change, merging, pulls, team_change};
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

/// `text` as one shell word in single quotes.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// `directory`, canonical, as one shell word.
fn word(directory: &Path) -> String {
    quote(&directory.canonicalize().unwrap().display().to_string())
}

/// `-C <directory>`, which reaches the board at `directory` from anywhere.
fn at(directory: &Path) -> String {
    format!("-C {}", word(directory))
}

/// `-C` naming the personal board, which holds the desk's change.
fn personal(desk: &Desk) -> String {
    at(desk.personal.workdir())
}

/// The `fetch` and `mount update` the warning tells the agent to run, with
/// `board` reaching the board that holds the mount.
fn by_hand(desk: &Desk, board: &str) -> String {
    format!(
        "git -C {} fetch 'origin' '+refs/heads/main:refs/remotes/origin/main' && board {board} \
         mount update '{MOUNT}'",
        word(&desk.checkout())
    )
}

/// The output of a merge of the change that exits 0, and its stderr.
fn merged(desk: &Desk) -> (Value, String) {
    let (ran, document) = desk.call("merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 0, "{document}");
    (document["data"]["output"].clone(), text(&ran.stderr))
}

/// Points the change at the mount `mount`.
fn mounting(desk: &Desk, mount: &str) {
    let mut record = desk.record(CHANGE).unwrap();
    record["mount"] = json!(mount);
    desk.write("changes/lever.json", &record.to_string());
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
        (
            &json!(WARNING),
            None,
            &json!(by_hand(&desk, &personal(&desk)))
        )
    );
    assert!(stderr.contains(WARNING), "{stderr}");
    assert_eq!(pin(&desk), before);
    pulls(&desk, true);
    let (result, _) = merged(&desk);
    assert_eq!(result["mount"]["new"], json!(commit), "{result}");
    assert_eq!(pin(&desk), commit);
}

#[test]
fn a_queued_merge_of_a_missing_mount_gives_both_causes_and_one_with_its_mount_only_the_queue() {
    for mount in ["/repos/gone", MOUNT] {
        let (desk, _) = ready(false);
        mounting(&desk, mount);
        let (result, _) = merged(&desk);
        let warning = &result["mount_warning"];
        let message = warning["message"].as_str().unwrap();
        assert!(
            message.contains("GitHub only queued the merge"),
            "{message}"
        );
        if mount == MOUNT {
            assert_eq!(warning.get("cause"), None, "{warning}");
            assert!(message.starts_with("GitHub only queued"), "{message}");
        } else {
            assert_eq!(warning["cause"], "board::path::not_found", "{warning}");
            let command = format!("board {} mount update '/repos/gone'", personal(&desk));
            assert_eq!(warning["command"], json!(command));
        }
    }
}

#[test]
fn a_mount_path_holding_shell_code_is_one_quoted_word_that_runs_nothing_and_a_plain_one_moves() {
    for injected in [true, false] {
        let (desk, commit) = ready(false);
        let root = desk.checkout().parent().unwrap().to_path_buf();
        let marker = root.join("marker");
        let mount = if injected {
            let marker = marker.display();
            format!("/repos/gone;touch {marker}$(touch {marker})")
        } else {
            MOUNT.to_owned()
        };
        mounting(&desk, &mount);
        let (result, _) = merged(&desk);
        let command = result["mount_warning"]["command"].as_str().unwrap();
        assert!(command.contains(&quote(&mount)), "{command}");
        let (code, _, stderr) = desk.process("sh", &root, &["-c", command], &[]);
        assert!(!marker.exists(), "{command} ran the injected part");
        if injected {
            assert_ne!(code, 0, "{command}");
        } else {
            assert_eq!(code, 0, "{command}: {stderr}");
            assert_eq!(pin(&desk), commit);
        }
    }
}

#[test]
fn a_merge_with_no_known_mount_gives_no_command_and_one_with_a_mount_does() {
    for known in [false, true] {
        let (desk, _) = ready(false);
        if !known {
            super::desk::remove(&desk.personal.workdir().join("repos/widgets.json"));
        }
        // With no mount of the checkout, only --personal finds a board.
        let flags = merging("Add a lever", "Levers lift.", &["--head", HEAD]);
        let (ran, document) = desk.call_in(&["--personal"], "merge", &flags);
        assert_eq!(ran.code, 0, "{document}");
        let warning = &document["data"]["output"]["mount_warning"];
        if known {
            assert_eq!(warning["command"], json!(by_hand(&desk, "--personal")));
        } else {
            assert_eq!(warning.get("command"), None, "{warning}");
            let message = warning["message"].as_str().unwrap();
            assert!(message.contains("mount update"), "{message}");
        }
    }
}

#[test]
fn the_command_runs_in_a_shell_for_a_clone_whose_directory_has_a_space_as_for_one_without() {
    for name in ["widgets clone", "widgets-clone"] {
        let (desk, commit) = ready(false);
        let root = desk.checkout().parent().unwrap().to_path_buf();
        let remote = desk.remote().display().to_string();
        desk.git(&root, &["clone", "-q", &remote, name]);
        let mut mount = desk.record(MOUNT).unwrap();
        mount["repo"] = json!(root.join(name));
        desk.write("repos/widgets.json", &mount.to_string());
        let (result, _) = merged(&desk);
        let command = result["mount_warning"]["command"].as_str().unwrap();
        assert!(command.contains(&word(&root.join(name))), "{command}");
        let (code, _, stderr) = desk.process("sh", &root, &["-c", command], &[]);
        assert_eq!(code, 0, "{name}: {command}: {stderr}");
        assert_eq!(pin(&desk), commit, "{name}");
    }
}

#[test]
fn the_command_names_the_board_that_holds_the_change_however_the_merge_reached_it() {
    for front in [&[][..], &["--personal"][..]] {
        let (desk, _) = ready(false);
        let (ran, document) = desk.call_in(front, "merge", &["--change", CHANGE]);
        assert_eq!(ran.code, 0, "{document}");
        assert_eq!(
            document["data"]["output"]["mount_warning"]["command"],
            json!(by_hand(&desk, &personal(&desk))),
            "{front:?}"
        );
    }
    let desk = Desk::new();
    let team = desk.team();
    team_change(&desk, &team, TEAM_TESTED);
    pulls(&desk, false);
    desk.gh_answers(&json!(""), 0, "");
    let (ran, document) = desk.call_in(&["-C", &team.path()], "merge", &["--change", CHANGE]);
    assert_eq!(ran.code, 0, "{document}");
    assert_eq!(
        document["data"]["output"]["mount_warning"]["command"],
        json!(by_hand(&desk, &at(team.workdir())))
    );
    let (desk, _) = ready(false);
    let result = desk.output(
        "merge",
        &merging("Add a lever", "Levers lift.", &["--head", HEAD]),
    );
    assert_eq!(
        result["mount_warning"]["command"],
        json!(by_hand(&desk, "--personal"))
    );
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
                    &json!(by_hand(&desk, &personal(&desk)))
                ),
            );
            assert_eq!(result.get("mount"), None);
            assert_eq!(pin(&desk), before);
        }
    }
}

#[test]
fn a_clone_whose_fetch_refspec_leaves_out_the_branch_still_moves_the_pin_as_one_that_has_it_does() {
    for refspec in ["+refs/heads/other:refs/remotes/origin/other", ""] {
        let (desk, commit) = ready(true);
        if !refspec.is_empty() {
            let checkout = desk.checkout();
            desk.git(&checkout, &["config", "remote.origin.fetch", refspec]);
        }
        let (result, _) = merged(&desk);
        assert_eq!(result["mount"]["new"], json!(commit), "{refspec}: {result}");
        assert_eq!(pin(&desk), commit);
    }
}

#[test]
fn a_change_naming_a_missing_mount_still_reaches_done_and_warns_and_its_twin_moves_the_mount() {
    for mount in ["/repos/gone", MOUNT] {
        let (desk, commit) = ready(true);
        let before = pin(&desk);
        mounting(&desk, mount);
        let (result, _) = merged(&desk);
        assert_eq!(result["phase"], "done", "{mount}");
        if mount == MOUNT {
            assert_eq!(result["mount"]["new"], json!(commit), "{result}");
            assert_eq!(pin(&desk), commit);
        } else {
            let warning = &result["mount_warning"];
            let command = format!("board {} mount update '/repos/gone'", personal(&desk));
            assert_eq!(
                (&warning["code"], &warning["cause"], &warning["command"]),
                (
                    &json!(WARNING),
                    &json!("board::path::not_found"),
                    &json!(command)
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
            let command = format!("board {} mount update '{MOUNT}'", personal(&desk));
            assert_eq!(
                (&warning["code"], warning.get("cause"), &warning["command"]),
                (&json!(WARNING), None, &json!(command)),
            );
            assert_eq!(result.get("mount"), None);
            assert_eq!(pin(&desk), before);
        } else {
            assert_eq!(result["mount"]["new"], json!(commit), "{result}");
            assert_eq!(pin(&desk), commit);
        }
    }
}
