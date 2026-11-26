//! The `paths` check: the change's plan declares paths and its diff can be
//! read. A file the diff edits that no planned path matches does not fail
//! it; the change adds that file to its plan's paths, and the passing
//! detail names it.

use super::{Observed, kinds};

/// What is missing for the `paths` check, or none when it passes.
pub(super) fn missing(observed: &Observed) -> Option<String> {
    if observed.planned.is_empty() {
        return Some(
            "the change's plan declares no paths, so every edited file is outside them; write the \
             plan again with `board changes plan` and its paths"
                .to_owned(),
        );
    }
    observed.diff_unread.clone()
}

/// What a passing `paths` check says: the files the change adds to its
/// plan's paths, at most ten of them, or nothing when there are none.
pub(super) fn passed(observed: &Observed) -> String {
    if observed.outside.is_empty() {
        return String::new();
    }
    let added: Vec<&str> = observed.outside.iter().map(String::as_str).collect();
    format!(
        "these files are outside the plan's paths, and the change adds them to its paths without \
         a new plan: {}. Their rules were not in the review brief. `board changes pr` lists them \
         in the pull request body.",
        kinds::bounded(&added)
    )
}
