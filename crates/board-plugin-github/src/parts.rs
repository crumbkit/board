//! The parts a push sends, which the boundary checks: the branch, each
//! commit's message, names, emails, headers, diff and binary paths, and
//! each annotated tag a ref peels through. README.md names every part.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::LazyLock;

use board_ops::{Host, PluginError};
use regex::Regex;
use serde_json::json;

use crate::git::{error, git, outgoing};

/// The short SHA that names a commit's or tag's parts, at a fixed length so
/// part names never change.
pub(crate) const SHORT: usize = 12;
/// The flags of `git show` that give a commit's patch against its first
/// parent with every file's bytes: no colour, external diff or textconv.
const DIFF: [&str; 5] = [
    "--format=",
    "--diff-merges=first-parent",
    "--no-color",
    "--no-ext-diff",
    "--no-textconv",
];
/// A tag object's tagger line: name, email, then the time and zone.
static TAGGER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^tagger (.*) <([^>]*)> \d+ [+-]\d{4}$").expect("the pattern is a constant")
});

/// Named texts, sorted by name, as the boundary takes them.
pub(crate) type Parts = BTreeMap<String, String>;

/// The commits a push of `head` sends past `have`, and the parts that name
/// `branch` and each commit. `github.push` and the pre-push hook both build
/// their parts here, so a push the action makes gives the hook the same
/// content to check.
pub(crate) fn push_parts(
    host: &dyn Host,
    at: &Path,
    branch: &str,
    head: &str,
    have: &[String],
) -> Result<(Vec<String>, Parts), PluginError> {
    let commits = outgoing(host, at, head, have)?;
    let mut parts = commit_parts(host, at, &commits)?;
    parts.insert("push.branch".to_owned(), branch.to_owned());
    Ok((commits, parts))
}

/// The parts of a push of object `sha` to the ref `name` past `have`: the
/// tags it peels through, then the commits. An annotated tag's message,
/// tagger name, tagger email and other headers, its own name among them,
/// are parts named `tag.<short sha>.<field>`, for each tag down to the
/// commit. A ref whose object is a tree or a blob is
/// `github::push::not_commit`.
pub(crate) fn ref_parts(
    host: &dyn Host,
    at: &Path,
    name: &str,
    sha: &str,
    have: &[String],
) -> Result<Parts, PluginError> {
    let mut parts = Parts::new();
    let mut sha = sha.to_owned();
    let mut kind = git(host, at, &["cat-file", "-t", &sha], "")?
        .trim()
        .to_owned();
    while kind == "tag" {
        let text = git(host, at, &["cat-file", "tag", &sha], "")?;
        let (head, message) = text.split_once("\n\n").unwrap_or((&text, ""));
        let tagger = head.lines().find_map(|line| TAGGER.captures(line));
        let field = |index| tagger.as_ref().map_or("", |found| &found[index]).to_owned();
        let prefix = format!("tag.{}.", &sha[..SHORT.min(sha.len())]);
        parts.insert(format!("{prefix}message"), strip_newline(message));
        parts.insert(format!("{prefix}tagger_name"), field(1));
        parts.insert(format!("{prefix}tagger_email"), field(2));
        parts.insert(
            format!("{prefix}headers"),
            headers(&text, &["object", "type", "tagger"]),
        );
        let object = head.lines().find_map(|line| line.strip_prefix("object "));
        object.unwrap_or_default().trim().clone_into(&mut sha);
        git(host, at, &["cat-file", "-t", &sha], "")?
            .trim()
            .clone_into(&mut kind);
    }
    if kind != "commit" {
        return Err(error(
            "github::push::not_commit",
            format!(
                "{name} names a {kind}, which the boundary cannot check as commits; push a \
                 branch or a tag of a commit"
            ),
            json!({"ref": name, "object": sha}),
        ));
    }
    let branch = name.strip_prefix("refs/heads/").unwrap_or(name);
    let (_, commits) = push_parts(host, at, branch, &sha, have)?;
    parts.extend(commits);
    Ok(parts)
}

/// Each commit's message, names, emails, headers, diff and binary paths, as
/// parts named `commit.<short sha>.<field>`. The diff is git's text of
/// every file, binary or not, with no textconv or external diff and with
/// file names unquoted, so it holds the bytes sent. A commit that changes a
/// file Git LFS stores is `github::push::lfs`.
pub(crate) fn commit_parts(
    host: &dyn Host,
    at: &Path,
    shas: &[String],
) -> Result<Parts, PluginError> {
    let mut parts = Parts::new();
    let unquoted = ["-c", "core.quotePath=false", "show"];
    for sha in shas {
        let numstat: Vec<&str> = (unquoted.iter().chain(&DIFF).copied())
            .chain(["--numstat", "-z", sha])
            .collect();
        let changed = changed(&git(host, at, &numstat, "")?);
        refuse_lfs(host, at, sha, &changed)?;
        let format = "--format=%an%x00%ae%x00%cn%x00%ce%x00%B";
        let shown = git(host, at, &["show", "-s", format, sha], "")?;
        let mut fields = shown.splitn(5, '\0');
        let prefix = format!("commit.{}.", &sha[..SHORT.min(sha.len())]);
        for name in [
            "author_name",
            "author_email",
            "committer_name",
            "committer_email",
        ] {
            let value = fields.next().unwrap_or_default().to_owned();
            parts.insert(format!("{prefix}{name}"), value);
        }
        let message = strip_newline(fields.next().unwrap_or_default());
        parts.insert(format!("{prefix}message"), message);
        let object = git(host, at, &["cat-file", "commit", sha], "")?;
        let skipped = ["tree", "parent", "author", "committer"];
        parts.insert(format!("{prefix}headers"), headers(&object, &skipped));
        let diff: Vec<&str> = (unquoted.iter().chain(&DIFF).copied())
            .chain(["--text", sha])
            .collect();
        parts.insert(format!("{prefix}diff"), git(host, at, &diff, "")?);
        let binary: String = (changed.iter())
            .filter(|(_, binary)| *binary)
            .flat_map(|(path, _)| [path.as_str(), "\n"])
            .collect();
        parts.insert(format!("{prefix}binary"), binary);
    }
    Ok(parts)
}

/// `text` without one trailing newline.
fn strip_newline(text: &str) -> String {
    text.strip_suffix('\n').unwrap_or(text).to_owned()
}

/// Each path `--numstat -z` names, and whether it counts as binary (`-` for
/// lines added and removed). A rename gives its target.
pub(crate) fn changed(numstat: &str) -> Vec<(String, bool)> {
    let mut paths = Vec::new();
    let mut fields = numstat.split('\0');
    while let Some(field) = fields.next() {
        let field = field.trim_start_matches('\n');
        let (added, rest) = field.split_once('\t').unwrap_or((field, ""));
        let (removed, path) = rest.split_once('\t').unwrap_or((rest, ""));
        let path = if path.is_empty() && !removed.is_empty() {
            fields.next();
            fields.next().unwrap_or_default()
        } else {
            path
        };
        if !path.is_empty() {
            paths.push((path.to_owned(), (added, removed) == ("-", "-")));
        }
    }
    paths
}

/// An object's header lines other than those `skipped` names, with their
/// continuations.
pub(crate) fn headers(text: &str, skipped: &[&str]) -> String {
    let head = text.split_once("\n\n").map_or(text, |(head, _)| head);
    let mut kept = String::new();
    let mut keep = false;
    for line in head.lines() {
        if !line.starts_with(' ') {
            let name = line.split(' ').next().unwrap_or_default();
            keep = !skipped.contains(&name);
        }
        if keep {
            kept.push_str(line);
            kept.push('\n');
        }
    }
    kept
}

/// `github::push::lfs` when any of the commit's paths is stored by Git LFS,
/// whose hook uploads bytes the boundary cannot check.
fn refuse_lfs(
    host: &dyn Host,
    at: &Path,
    sha: &str,
    changed: &[(String, bool)],
) -> Result<(), PluginError> {
    if changed.is_empty() {
        return Ok(());
    }
    let stdin: String = (changed.iter())
        .flat_map(|(path, _)| [path.as_str(), "\0"])
        .collect();
    let source = format!("--source={sha}");
    let arguments = ["check-attr", "-z", &source, "--stdin", "filter"];
    let answer = git(host, at, &arguments, &stdin)?;
    let fields: Vec<&str> = answer.split('\0').collect();
    let stored: Vec<&str> = (fields.chunks(3))
        .filter(|triple| triple.len() == 3 && triple[2] == "lfs")
        .map(|triple| triple[0])
        .collect();
    if stored.is_empty() {
        return Ok(());
    }
    Err(error(
        "github::push::lfs",
        format!(
            "commit {} changes {}, which Git LFS stores; its hook uploads those files' bytes \
             where the boundary cannot check them, so nothing was pushed; stop tracking them \
             with `git lfs untrack` and commit them as plain files, or ask the owner how to \
             send them",
            &sha[..SHORT.min(sha.len())],
            stored.join(", ")
        ),
        json!({"commit": sha, "paths": stored}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numstat_names_each_path_a_rename_by_its_target_and_binary_files_by_their_dashes() {
        let numstat = "1\t0\ta.txt\0-\t-\timage.png\0\n0\t0\t\0old.txt\0new.txt\0";
        assert_eq!(
            changed(numstat),
            [
                ("a.txt".to_owned(), false),
                ("image.png".to_owned(), true),
                ("new.txt".to_owned(), false)
            ]
        );
    }

    #[test]
    fn headers_keep_the_lines_not_skipped_with_their_continuations() {
        let commit = "tree t\nparent p\nauthor a\ncommitter c\nencoding x\nmergetag object o\n \
                      tag v1\n\nThe message\n";
        assert_eq!(
            headers(commit, &["tree", "parent", "author", "committer"]),
            "encoding x\nmergetag object o\n tag v1\n"
        );
        assert_eq!(headers("tree t\n\nmessage\n", &["tree"]), "");
    }
}
