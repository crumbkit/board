use super::*;

fn hint(code: Code) -> String {
    code.hint(&Fields::default())
}

#[test]
fn the_rewound_hint_says_the_clones_commits_are_pushed_again_and_a_deliberate_rewrite_needs_a_fresh_clone()
 {
    let text = hint(Code::SyncRewound);
    assert!(text.contains("pushed again"), "{text}");
    assert!(text.contains("removed"), "{text}");
    assert!(text.contains("clone the board again"), "{text}");
}

#[test]
fn the_rewound_hint_keeps_the_old_clone_until_its_unpushed_commits_are_checked() {
    let text = hint(Code::SyncRewound);
    assert!(text.contains("keep the old clone"), "{text}");
    assert!(text.contains("git log <remote>/<branch>..HEAD"), "{text}");
    assert!(text.contains("Before fetching in it"), "{text}");
    assert!(
        text.contains("after a fetch the same command also lists the commits the rewrite dropped"),
        "{text}"
    );
}

#[test]
fn the_merging_hint_names_the_commands_of_every_operation_it_refuses_for() {
    let text = hint(Code::SyncMerging);
    for operation in ["merge", "rebase", "cherry-pick", "revert"] {
        for end in ["--continue", "--abort"] {
            let command = format!("git {operation} {end}");
            assert!(text.contains(&command), "{command}: {text}");
        }
    }
}

#[test]
fn the_merging_hint_gives_the_code_a_note_write_and_a_task_write_fails_with() {
    let text = hint(Code::SyncMerging);
    assert!(text.contains("`notes::board::refused`"), "{text}");
    assert!(text.contains("`tasks::board::refused`"), "{text}");
    assert!(text.contains("`board::put::locked`"), "{text}");
}

#[test]
fn the_commit_hint_says_a_log_through_a_mount_lists_commits_get_at_on_the_board_cannot_read() {
    let text = hint(Code::CommitNotFound);
    assert!(
        text.contains("through a mount lists the mounted repository's commits"),
        "{text}"
    );
    assert!(
        text.contains("`get --at` on this board cannot read"),
        "{text}"
    );
    assert!(
        text.contains("board log of the directory that holds the mount record lists the board commits that moved its pin, which --at takes"),
        "{text}"
    );
    assert!(!text.contains("lists the board's commits"), "{text}");
}

#[test]
fn the_locked_hint_says_board_clears_a_lock_a_killed_board_left_and_a_person_clears_the_rest() {
    let text = hint(Code::PutLocked);
    assert!(text.contains("a killed Board"), "{text}");
    assert!(text.contains("the next put or sync removes"), "{text}");
    assert!(text.contains("any other lock file"), "{text}");
}

#[test]
fn the_no_branch_hint_names_the_way_out_of_each_case() {
    let text = hint(Code::SyncNoBranch);
    assert!(text.contains("without `--no-push`"), "{text}");
    assert!(text.contains("board plugin enable tasks"), "{text}");
    assert!(text.contains("git push <remote> HEAD:<branch>"), "{text}");
    assert!(!text.contains("ssh-add"), "{text}");
}

#[test]
fn the_unnamed_personal_board_hint_names_init_personal_before_the_hand_written_file() {
    let text = hint(Code::SettingsNoPersonal);
    let init = text.find("`board init --personal`").expect(&text);
    let file = text.find("~/.config/board/config.json").expect(&text);
    assert!(init < file, "{text}");
}

#[test]
fn the_personal_elsewhere_hint_says_the_named_board_stays_and_how_to_move_it_by_hand() {
    let text = hint(Code::ConfigPersonalElsewhere);
    assert!(text.contains("edit `personal`"), "{text}");
    assert!(text.contains("by hand"), "{text}");
    assert!(!text.contains("`board "), "{text}");
}

#[test]
fn the_unreadable_reference_hint_says_to_check_again_then_repair_the_mount_and_never_to_delete() {
    assert_eq!(
        hint(Code::RefUnreadable),
        "the record is not known to be broken; run `board check <path>` again, and if it \
         still fails, run `board check` on the mount the message names, whose hint says how \
         to repair it, such as `board mount update <path>`; do not delete the record"
    );
    let fields = Fields::at(&"/vendor".parse().unwrap());
    assert_eq!(
        Code::RefUnreadable.hint(&fields),
        "the record is not known to be broken; Board could not read /vendor, which it \
         references; run `board check /vendor`, whose hint says how to make it readable, then \
         check the record again; do not delete the record"
    );
}
