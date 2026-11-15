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
fn the_merging_hint_names_the_commands_of_every_operation_it_refuses_for() {
    let text = hint(Code::SyncMerging);
    for operation in ["merge", "rebase", "cherry-pick", "revert"] {
        for end in ["--continue", "--abort"] {
            let command = format!("git {operation} {end}");
            assert!(text.contains(&command), "{command}: {text}");
        }
    }
}
