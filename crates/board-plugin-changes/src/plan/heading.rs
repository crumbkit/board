//! The check of a plan's first `# ` heading against the change it is for.
//!
//! A plan headed by another change's title most likely holds that change's
//! plan, such as when two agents build their input in one shared file. The
//! check finds the other change by the name `changes.start` derives from the
//! heading, so it costs one read; a change started with another `--name` is
//! not found.

use board_ops::{Host, PluginError};
use serde_json::{Value, json};

use crate::board::get;
use crate::gates::FENCES;
use crate::start::name_of;
use crate::{Warning, refusal};

/// The first `# ` heading of `text` outside a code fence, trimmed; none
/// when it has none.
fn first_heading(text: &str) -> Option<&str> {
    let mut fenced = false;
    (text.lines())
        .find_map(|line| {
            if FENCES
                .iter()
                .any(|fence| line.trim_start().starts_with(fence))
            {
                fenced = !fenced;
                return None;
            }
            if fenced {
                None
            } else {
                line.strip_prefix("# ")
            }
        })
        .map(str::trim)
}

/// Refuses with `changes::plan::other_change` a plan for the change at
/// `path`, titled `title`, whose first heading is the title of another
/// change that is not dropped. Warns with `changes::plan::title_differs`
/// when the first heading differs from `title` in any other way.
pub(crate) fn check(
    host: &dyn Host,
    path: &str,
    title: &str,
    text: &str,
) -> Result<Option<Warning>, PluginError> {
    let Some(heading) = first_heading(text) else {
        return Ok(None);
    };
    if heading == title.trim() {
        return Ok(None);
    }
    let other = format!("/changes/{}", name_of(heading));
    if other != path {
        let held = get(host, std::slice::from_ref(&other))?
            .into_iter()
            .next()
            .flatten();
        let record: Value =
            (held.and_then(|bytes| serde_json::from_slice(&bytes).ok())).unwrap_or(Value::Null);
        let titled = record.get("title").and_then(Value::as_str).map(str::trim) == Some(heading);
        let dropped = record.get("phase").and_then(Value::as_str) == Some("dropped");
        if titled && !dropped {
            return Err(refusal(
                "changes::plan::other_change",
                format!(
                    "the plan's first heading \"{heading}\" is the title of {other}, so its text \
                     looks like that change's plan; send the plan written for {path}, or make its \
                     first heading this change's title \"{title}\""
                ),
                [("path", json!(path)), ("other", json!(other))],
            ));
        }
    }
    Ok(Some(Warning {
        code: "changes::plan::title_differs",
        detail: format!(
            "the plan's first heading \"{heading}\" differs from the change's title \"{title}\"; \
             check that the text is this change's plan"
        ),
    }))
}

#[cfg(test)]
mod tests {
    use super::first_heading;

    #[test]
    fn the_first_heading_is_the_first_level_one_heading() {
        assert_eq!(
            first_heading("intro\n## Problem\n#  Title  \n# Later\n"),
            Some("Title")
        );
        assert_eq!(first_heading("## Problem\n"), None);
    }

    #[test]
    fn a_line_inside_a_code_fence_is_never_the_first_heading() {
        assert_eq!(
            first_heading("## Steps\n```sh\n# install the tool\n```\n~~~\n# not\n~~~\n# Title\n"),
            Some("Title")
        );
        assert_eq!(first_heading("```\n# install the tool\n```\n"), None);
    }
}
