//! Counting words and finding `@path` imports in Markdown.

/// The number of whitespace-separated words in `text`.
pub(crate) fn count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// The paths `text` imports with Claude Code's `@path` syntax, in order: every
/// whitespace-separated token that starts with `@`, outside fenced code blocks
/// and inline code spans.
pub(crate) fn imports(text: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for token in outside_code_spans(line).split_whitespace() {
            if let Some(path) = token.strip_prefix('@')
                && !path.is_empty()
            {
                imports.push(path.to_owned());
            }
        }
    }
    imports
}

/// `line` with every inline code span, and its backticks, replaced by a space.
fn outside_code_spans(line: &str) -> String {
    let mut kept = String::with_capacity(line.len());
    let mut in_span = false;
    for character in line.chars() {
        if character == '`' {
            in_span = !in_span;
            kept.push(' ');
        } else if !in_span {
            kept.push(character);
        }
    }
    kept
}

#[cfg(test)]
mod tests;
