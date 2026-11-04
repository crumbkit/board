//! Which pairs of GLOBs some one path matches, and which none does.

use super::*;

#[test]
fn two_globs_overlap_when_some_path_matches_both_and_not_when_none_can() {
    let pairs = [
        ("repos/inbox/crates/new/**", "repos/inbox/*/*.rs", true),
        ("crates/store/**", "*/Dockerfile*", true),
        ("crates/store/src/*.rs", "*/Dockerfile*", true),
        ("crates/store/src/*.rs", "*.toml", false),
        ("crates/new/src/*.rs", "crates/*.rs", true),
        ("docs/*", "crates/*.rs", false),
        ("a.rs", "a.rs", true),
        ("a.rs", "b.rs", false),
        ("a?c", "abc", true),
        ("a?c", "ac", false),
        ("[ab]x", "cx", true),
        ("*", "", true),
        ("x*", "", false),
        ("*a*", "*b*", true),
        ("a*", "*b", true),
        ("a*", "b*", false),
    ];
    for (one, other, expected) in pairs {
        assert_eq!(overlap(one, other), expected, "{one} and {other}");
        assert_eq!(overlap(other, one), expected, "{other} and {one}");
    }
}
