//! `list`: the prompts in the repos in scope, read at their pins, by repo
//! then title.

use std::collections::BTreeMap;

use board_ops::{Host, Origin, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::prompts::{listed_files, query};
use crate::repos::{self, json_body};

/// The most prompts `list` shows; `total` counts the rest (STANDARDS.md 23).
pub(crate) const SHOWN: usize = 100;

/// The prompts to list, in the repos the caller's remote chooses.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ListInput")]
pub(crate) struct Input {
    #[schemars(
        regex(pattern = "^/"),
        description = "One repo's prompts: / for the board's own tree, or a marked repo's mount \
                       path such as /repos/notes-personal; the caller's git remote chooses the \
                       repos when left out."
    )]
    pub(crate) repo: Option<String>,
    #[schemars(
        length(min = 1),
        description = "A git remote, or owner/name, to choose the repos by in place of the \
                       origin of the directory board was started in."
    )]
    pub(crate) remote: Option<String>,
}

#[derive(Debug, Serialize, JsonSchema)]
#[schemars(
    deny_unknown_fields,
    description = "A prompt as written: where it is, what it is for, where it runs and the \
                   inputs it takes."
)]
pub(crate) struct ListedPrompt {
    pub(crate) repo: String,
    pub(crate) path: String,
    pub(crate) title: String,
    pub(crate) purpose: String,
    pub(crate) runs_in: BTreeMap<String, String>,
    pub(crate) inputs: BTreeMap<String, String>,
}

/// At most 100 prompts, by repo then title, and how many there are.
///
/// `degraded` counts the Markdown files under a `prompts/` directory in scope
/// that `board check` marks degraded for any code but a broken link
/// (`board::ref::dangling` or `board::ref::wrong_type`): an invalid prompt, or
/// frontmatter that does not parse or names an unknown type. Plain Markdown,
/// such as an eval's evidence, is not counted. `repos` are the repos read. `origin_unread`
/// says why the caller's git repository gave no origin, when it did not, so
/// only `/` and the personal repo were read.
#[derive(Debug, Serialize, JsonSchema)]
#[schemars(rename = "ListOutput", deny_unknown_fields)]
pub(crate) struct Output {
    pub(crate) prompts: Vec<ListedPrompt>,
    pub(crate) total: usize,
    pub(crate) degraded: usize,
    pub(crate) repos: Vec<String>,
    // Required, and null when the origin was read or not asked for.
    pub(crate) origin_unread: Option<String>,
}

pub(crate) fn list(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let given: Input = PluginError::read_input("prompts", "list", input)?;
    let (remote, unread) = match (&given.repo, &given.remote) {
        (None, None) => match host.caller_remote() {
            Origin::Outside => (None, None),
            Origin::Url(url) => (Some(url), None),
            Origin::Unread(why) => (None, Some(why)),
        },
        (_, remote) => (remote.clone(), None),
    };
    let read = query(host, &listed_files())?;
    let roots = repos::scope(
        "prompts",
        &read.found,
        given.repo.as_deref(),
        remote.as_deref(),
        unread.as_deref(),
    )?;
    let mut prompts = Vec::new();
    let mut degraded = 0;
    for file in &read.files {
        let Some(repo) = read
            .roots
            .get(&file.source)
            .filter(|repo| roots.contains(repo))
        else {
            continue;
        };
        if file.degraded() {
            degraded += 1;
            continue;
        }
        let record = json_body(&file.frontmatter);
        let strings = |value: &Value| -> BTreeMap<String, String> {
            serde_json::from_value(value.clone()).unwrap_or_default()
        };
        prompts.push(ListedPrompt {
            repo: repo.clone(),
            path: file.logical.clone(),
            title: repos::shown(&record["title"]),
            purpose: repos::shown(&record["purpose"]),
            runs_in: strings(&record["runs_in"]),
            inputs: strings(&record["inputs"]),
        });
    }
    prompts.sort_by(|a, b| (&a.repo, &a.title, &a.path).cmp(&(&b.repo, &b.title, &b.path)));
    let total = prompts.len();
    prompts.truncate(SHOWN);
    Ok(json!(Output {
        prompts,
        total,
        degraded,
        repos: roots,
        origin_unread: unread,
    }))
}
