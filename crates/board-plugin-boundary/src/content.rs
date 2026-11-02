//! The content a check covers: its digest, which names its crossing, and
//! the text the owner is shown when asked to let it through.

use std::collections::BTreeMap;

use board_model::Digest;
use serde_json::json;

/// What leaves a checkout: the action that sends it, where it goes, and
/// each part by its name.
pub(crate) struct Content<'a> {
    pub(crate) action: &'a str,
    pub(crate) to: &'a str,
    pub(crate) parts: &'a BTreeMap<String, String>,
}

impl Content<'_> {
    /// The SHA-256, in lowercase hex, of the UTF-8 JSON
    /// `{"action", "parts", "to"}` with keys sorted, no whitespace and
    /// characters unescaped, as Python's
    /// `json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)`
    /// writes it.
    pub(crate) fn digest(&self) -> String {
        let canonical = json!({"action": self.action, "parts": self.parts, "to": self.to});
        Digest::of(canonical.to_string().as_bytes()).hex()
    }

    /// The content as the owner's approval shows it: the action,
    /// destination and each part name quoted as JSON, and every line of a
    /// part indented by two spaces. No part's line starts in the first
    /// column, and a quoted name holds no line break, so no two contents
    /// show the same text.
    pub(crate) fn shown(&self) -> String {
        let mut lines = vec![
            format!("action {}", quoted(self.action)),
            format!("to {}", quoted(self.to)),
        ];
        for (name, body) in self.parts {
            lines.push(String::new());
            lines.push(format!("part {}", quoted(name)));
            lines.extend(body.split('\n').map(|line| format!("  {line}")));
        }
        lines.join("\n") + "\n"
    }
}

/// `text` as a JSON string.
pub(crate) fn quoted(text: &str) -> String {
    json!(text).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content<'a>(
        action: &'a str,
        to: &'a str,
        parts: &'a BTreeMap<String, String>,
    ) -> Content<'a> {
        Content { action, to, parts }
    }

    fn parts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        (pairs.iter())
            .map(|(name, body)| ((*name).to_owned(), (*body).to_owned()))
            .collect()
    }

    #[test]
    fn the_digest_is_the_sha256_of_the_canonical_json_python_writes() {
        // python3 -c 'import hashlib, json; print(hashlib.sha256(json.dumps({"action": "github.pr-create",
        //   "parts": {"pr.body": "Fïne.\n\"q\"\t\x01", "pr.title": "T"}, "to": "github:octo/board"},
        //   sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()).hexdigest())'
        let parts = parts(&[("pr.title", "T"), ("pr.body", "Fïne.\n\"q\"\t\u{1}")]);
        let content = content("github.pr-create", "github:octo/board", &parts);
        assert_eq!(
            content.digest(),
            "38d2eafae704c1752d244eb85318dbb8bdc10e6702d161c93dc95800bbfd1482"
        );
    }

    #[test]
    fn the_owner_is_shown_each_part_so_that_no_two_contents_show_the_same_text() {
        let two = parts(&[("pr.body", "Fine.\n\nSee acme-123."), ("pr.title", "T")]);
        assert_eq!(
            content("github.pr-create", "github:octo/board", &two).shown(),
            "action \"github.pr-create\"\nto \"github:octo/board\"\n\npart \"pr.body\"\n  Fine.\n  \n  See acme-123.\n\npart \"pr.title\"\n  T\n"
        );
        let split = parts(&[("pr.body", "x"), ("pr.title", "y")]);
        let split = content("a", "t", &split).shown();
        for body in ["x\n\npr.title:\ny", "x\n\npart \"pr.title\"\n  y"] {
            let one = parts(&[("pr.body", body)]);
            assert_ne!(content("a", "t", &one).shown(), split, "{body:?}");
        }
        let one = parts(&[("p", "x")]);
        assert_ne!(
            content("a\nto \"t\"", "t", &one).shown(),
            content("a", "t", &one).shown()
        );
    }
}
