//! The absolute board paths a Markdown body names, which a share copies
//! when they hold a record or a file in this board's own tree.

use std::collections::BTreeSet;

use board_model::LogicalPath;

/// Each absolute board path that `text`, a Markdown file, names in its
/// body, sorted and once each.
///
/// A path is a token, split at whitespace, brackets, angle brackets,
/// quotes and backticks, that starts with one `/`. Its query or fragment
/// and trailing `.,;:!` are dropped, and so is a final `.json`, which a
/// record's path leaves out. So a path in prose, in a code span and as a
/// link's destination are all named. The root and a token that is not a
/// logical path, such as `/a/../b`, are not. Frontmatter that claims a
/// `$type` is skipped, because the record's schema says which of its
/// fields are references.
pub(crate) fn named(text: &str) -> Vec<LogicalPath> {
    let separators = |char: char| char.is_whitespace() || "()[]<>`\"'".contains(char);
    let found: BTreeSet<LogicalPath> = (body(text).split(separators))
        .filter_map(|token| {
            let token = token.find(['?', '#']).map_or(token, |end| &token[..end]);
            let token = token.trim_end_matches(['.', ',', ';', ':', '!']);
            let rest = token.strip_prefix('/')?;
            if rest.is_empty() || rest.starts_with('/') {
                return None;
            }
            LogicalPath::of_physical(rest).ok()
        })
        .collect();
    found.into_iter().collect()
}

/// The text after frontmatter that claims a `$type`, as Board reads a
/// Markdown record (board-model's `frontmatter::body`, which it does not
/// export): a first line `---`, a line opening `$type:`, and a closing
/// line `---` or `...`. All of `text` when it has no such block.
fn body(text: &str) -> &str {
    let unmarked = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = (unmarked.strip_prefix("---\n")).or_else(|| unmarked.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut at = 0;
    for line in rest.split_inclusive('\n') {
        let bare = line.trim_end_matches('\n').trim_end_matches('\r');
        if bare == "---" || bare == "..." {
            let claimed = rest[..at].lines().any(|line| line.starts_with("$type:"));
            return if claimed {
                &rest[at + line.len()..]
            } else {
                text
            };
        }
        at += line.len();
    }
    text
}

#[cfg(test)]
mod tests;
