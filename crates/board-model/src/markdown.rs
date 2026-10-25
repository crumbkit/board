//! Links in Markdown files, which are references (docs/architecture.md,
//! Typed references).

use percent_encoding::percent_decode_str;
use pulldown_cmark::{CowStr, Event, LinkType, Options, Parser, Tag};

use crate::frontmatter::body;
use crate::path::LogicalPath;

/// The destination of each inline, reference and image link in `bytes`, as
/// written and in document order, that names a board path: not an autolink,
/// whether a URL or an email address, one with a URL scheme, one starting
/// `//`, or a fragment alone. A footnote is not a link, and neither is
/// text in the frontmatter. Bytes that are not UTF-8 have none.
pub fn links(bytes: &[u8]) -> Vec<String> {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Vec::new();
    };
    let text = body(text);
    // Without footnotes, `[^1]: Ibid.` would define a link to `Ibid.`.
    Parser::new_ext(text, Options::ENABLE_FOOTNOTES)
        .filter_map(|event| match event {
            Event::Start(Tag::Link {
                link_type: LinkType::Autolink | LinkType::Email,
                ..
            }) => None,
            Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                Some(dest_url)
            }
            _ => None,
        })
        .filter(|destination| {
            let path = path_of(destination);
            !path.is_empty() && !path.starts_with("//") && !has_scheme(path)
        })
        .map(CowStr::into_string)
        .collect()
}

/// The logical path that `destination`, a link in the file at `file`,
/// reaches. It is read as a URL path is read against the file's own: from
/// the root when absolute, else from the file's directory, with `.` and
/// `..` segments removed and `..` stopping at the root (RFC 3986 5.2). Each
/// segment is percent-decoded first, as browsers read it, so `%2e%2e` is
/// `..`. The fragment and query are dropped, and so is a final `.json`, which
/// a record's path leaves out. `None` when a segment decodes to a `/` or to
/// bytes that are not UTF-8, or the result is not a logical path.
pub fn reached(file: &LogicalPath, destination: &str) -> Option<LogicalPath> {
    let path = path_of(destination);
    let mut segments: Vec<String> = Vec::new();
    if !path.starts_with('/') {
        segments.extend(file.segments().map(str::to_owned));
        segments.pop();
    }
    for segment in path.split('/') {
        let decoded = percent_decode_str(segment).decode_utf8().ok()?;
        match &*decoded {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            decoded if decoded.contains('/') => return None,
            decoded => segments.push(decoded.to_owned()),
        }
    }
    if let Some(last) = segments.last_mut()
        && let Some(stem) = last.strip_suffix(".json")
    {
        *last = stem.to_owned();
    }
    format!("/{}", segments.join("/")).parse().ok()
}

/// `destination` without its query and fragment.
fn path_of(destination: &str) -> &str {
    destination
        .find(['?', '#'])
        .map_or(destination, |end| &destination[..end])
}

/// Whether `path` starts with a URL scheme (RFC 3986 3.1): a letter, then
/// letters, digits, `+`, `-` or `.`, then `:`.
fn has_scheme(path: &str) -> bool {
    path.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|first: char| first.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|next| next.is_ascii_alphanumeric() || matches!(next, '+' | '-' | '.'))
    })
}

#[cfg(test)]
mod tests;
