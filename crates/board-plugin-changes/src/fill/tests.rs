//! The branch a mount follows, as `fill` names it from git's output.

use super::branch_named;

#[test]
fn a_local_or_remote_branch_names_the_branch_and_a_tag_or_commit_names_none() {
    let named = |shown: &str| branch_named(shown);
    assert_eq!(
        named("--end-of-options\nrefs/heads/main\n"),
        Some("main".to_owned())
    );
    assert_eq!(
        named("--end-of-options\nrefs/remotes/origin/release/1.0\n"),
        Some("release/1.0".to_owned())
    );
    assert_eq!(named("--end-of-options\nrefs/tags/v1\n"), None);
    assert_eq!(named("--end-of-options\n\n"), None);
}
