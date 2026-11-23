//! The edit of a task's refs and the board a ref's target is in.

use super::*;

fn paths(list: &[&str]) -> Vec<String> {
    list.iter().map(|path| (*path).to_owned()).collect()
}

#[test]
fn an_edit_appends_once_removes_and_keeps_the_order_of_the_rest() {
    let before = paths(&["/a", "/b"]);
    assert_eq!(
        edited(&before, &paths(&["/b", "/c", "/c"]), Some("/a"), 2),
        Ok(paths(&["/b", "/c"]))
    );
    assert_eq!(edited(&before, &[], Some("/z"), 2), Ok(before.clone()));
    assert_eq!(edited(&[], &[], None, 2), Ok(Vec::new()));
}

#[test]
fn an_edit_past_the_limit_is_refused_with_its_count_and_one_at_the_limit_is_admitted() {
    let before = paths(&["/a", "/b"]);
    assert_eq!(edited(&before, &paths(&["/c"]), None, 2), Err(3));
    assert_eq!(
        edited(&before, &paths(&["/c"]), Some("/a"), 2),
        Ok(paths(&["/b", "/c"]))
    );
}

#[test]
fn a_target_is_in_the_repos_board_only_when_it_crosses_exactly_the_mounts_to_the_root() {
    let nested = "/team/repos/x";
    assert_eq!(foreign("/", &[]), None);
    assert_eq!(foreign("/repos/x", &paths(&["/repos/x"])), None);
    assert_eq!(foreign(nested, &paths(&["/team", nested])), None);
    assert_eq!(
        foreign(nested, &paths(&["/team"])),
        Some("/team".to_owned())
    );
    assert_eq!(foreign(nested, &[]), Some("/".to_owned()));
    assert_eq!(
        foreign(nested, &paths(&["/team", nested, "/team/repos/x/y"])),
        Some("/team/repos/x/y".to_owned())
    );
    assert_eq!(
        foreign("/", &paths(&["/repos/x"])),
        Some("/repos/x".to_owned())
    );
    assert_eq!(
        foreign("/repos/x", &paths(&["/repos/xy"])),
        Some("/repos/xy".to_owned())
    );
}
