//! What a mounted repository says about contributing: its code owners, its
//! contributing docs and its check suite.

use std::sync::LazyLock;

use board_ops::{Host, PluginError, literal};
use regex::Regex;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::json;

use crate::board::{self, rows, text};
use crate::records::Route;
use crate::refusal;

const SUITE_TYPE: &str = "runs/suite/v1";
/// Where GitHub looks for CODEOWNERS, in its order; the first found is used.
pub(crate) const CODEOWNERS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];
/// The contributing docs, in the order a check suite is looked for in them.
pub(crate) const DOCS: [&str; 4] = [
    "CONTRIBUTING.md",
    ".github/CONTRIBUTING.md",
    "docs/CONTRIBUTING.md",
    "AGENTS.md",
];
/// A logical path written as inline code, such as `/suites/board-tests`.
static CODE_PATH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"`(/[A-Za-z0-9._/-]+)`").expect("the pattern is a constant; a test compiles it")
});

/// One CODEOWNERS line: a pattern and the owners of the files it matches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) struct Rule {
    pub(crate) pattern: String,
    pub(crate) owners: Vec<String>,
}

/// The text of each of `names` in `repo` that exists, by its logical path,
/// in the order of `names`.
pub(crate) fn files(
    host: &dyn Host,
    repo: &str,
    names: &[&str],
) -> Result<Vec<(String, String)>, PluginError> {
    let paths: Vec<String> = names.iter().map(|name| format!("{repo}/{name}")).collect();
    let found = board::get(host, &paths)?;
    Ok((paths.into_iter().zip(found))
        .filter_map(|(path, bytes)| Some((path, String::from_utf8_lossy(&bytes?).into_owned())))
        .collect())
}

/// The rules of a CODEOWNERS file, in order, without comments.
pub(crate) fn owners(text: &str) -> Vec<Rule> {
    (text.lines())
        .filter_map(|line| {
            let mut words = line
                .split('#')
                .next()
                .unwrap_or_default()
                .split_whitespace();
            let pattern = words.next()?.to_owned();
            Some(Rule {
                pattern,
                owners: words.map(str::to_owned).collect(),
            })
        })
        .collect()
}

/// The patterns someone other than `handle` owns, each once, in order.
pub(crate) fn ask_first(rules: &[Rule], handle: Option<&str>) -> Vec<String> {
    let mine = handle.map(str::to_lowercase).unwrap_or_default();
    let mut asking: Vec<String> = Vec::new();
    for rule in rules {
        let theirs = !rule.owners.is_empty()
            && !(rule.owners.iter()).any(|owner| owner.to_lowercase() == mine);
        if theirs && !asking.contains(&rule.pattern) {
            asking.push(rule.pattern.clone());
        }
    }
    asking
}

/// `unowned` when nothing names owners, `owner` when nobody else owns a
/// pattern, else `ask-first`.
pub(crate) fn route(rules: &[Rule], asking: &[String]) -> Route {
    if rules.iter().all(|rule| rule.owners.is_empty()) {
        Route::Unowned
    } else if asking.is_empty() {
        Route::Owner
    } else {
        Route::AskFirst
    }
}

/// The first `runs/suite/v1` the docs name as inline code: in the
/// repository's own board, or else in this one.
pub(crate) fn suite_in(
    host: &dyn Host,
    repo: &str,
    docs: &[(String, String)],
) -> Result<Option<String>, PluginError> {
    let candidates: Vec<String> = (docs.iter())
        .flat_map(|(_, text)| CODE_PATH.captures_iter(text))
        .filter_map(|captures| Some(captures.get(1)?.as_str().to_owned()))
        .flat_map(|path| [format!("{repo}{path}"), path])
        .collect();
    if candidates.is_empty() {
        return Ok(None);
    }
    let listed: Vec<String> = candidates.iter().map(|each| literal(each)).collect();
    let sql = format!(
        "SELECT logical FROM records WHERE type = {} AND logical IN ({})",
        literal(SUITE_TYPE),
        listed.join(", ")
    );
    let suites: Vec<String> = (rows(host, &sql)?.iter())
        .filter_map(|row| text(row.first()).map(str::to_owned))
        .collect();
    Ok(candidates.into_iter().find(|each| suites.contains(each)))
}

/// The check suite the repository's contributing docs name, or none.
pub(crate) fn check_suite(host: &dyn Host, repo: &str) -> Result<Option<String>, PluginError> {
    let docs = files(host, repo, &DOCS)?;
    suite_in(host, repo, &docs)
}

/// Refuses a mount path that holds nothing with `changes::mount::not_found`.
pub(crate) fn exists(host: &dyn Host, mount: &str) -> Result<(), PluginError> {
    if board::resolve(host, mount)?.is_some() {
        return Ok(());
    }
    Err(refusal(
        "changes::mount::not_found",
        format!(
            "nothing is at {mount}; name the repository by the logical path of its mount, such as \
             /repos/<name>, and see what is mounted with `board ls /repos`"
        ),
        [("mount", json!(mount))],
    ))
}
