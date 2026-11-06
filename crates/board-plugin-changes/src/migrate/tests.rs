//! How migrate reads a GitHub page's URL.

use super::github_ref;

#[test]
fn a_github_pull_request_or_issue_page_names_its_reference_and_another_url_none() {
    assert_eq!(
        github_ref("https://github.com/crumbkit/board/pull/12/"),
        Some(("crumbkit/board#12".to_owned(), true))
    );
    assert_eq!(
        github_ref("https://github.com/o/r/issues/3"),
        Some(("o/r#3".to_owned(), false))
    );
    assert_eq!(
        github_ref("https://gitlab.com/o/r/-/merge_requests/1"),
        None
    );
}
