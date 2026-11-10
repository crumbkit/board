//! The actions that send content to GitHub: each collects its parts, has
//! the boundary check them, and sends only on a pass. README.md names every
//! part, so a boundary's globs can name them.

use std::path::PathBuf;

use board_ops::{Host, PluginError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::crossing::{self, Passed};
use crate::gh;
use crate::git;
use crate::hook::{Handed, Pushing};
use crate::objects::{Kind, Object, write};
use crate::parts::{Parts, push_parts};
use crate::remote;

/// Where a call sends from and to: the personal board, the checkout's root,
/// its GitHub repository, and its mount in the personal board, if any.
pub(crate) struct Origin<'a> {
    host: &'a dyn Host,
    pub(crate) personal: Box<dyn Host + 'a>,
    pub(crate) top: PathBuf,
    pub(crate) owner: String,
    pub(crate) repo: String,
    mount: Option<String>,
}

impl Origin<'_> {
    /// The destination the boundary matches, in lower case.
    fn to(&self) -> String {
        format!("github:{}/{}", self.owner, self.repo)
    }

    /// The boundary's check of the parts of `github.<action>`.
    pub(crate) fn check(&self, action: &str, parts: &Parts) -> Result<Passed, PluginError> {
        let action = format!("github.{action}");
        crossing::check(
            &*self.personal,
            &action,
            &self.to(),
            self.mount.as_deref(),
            parts,
        )
    }

    /// Records `result` on the crossing, and returns the output every action
    /// shares.
    pub(crate) fn sent(&self, passed: Passed, result: String) -> Result<Delivered, PluginError> {
        crossing::record(&*self.personal, &passed.digest, &result)
            .map_err(|error| sent_already(&result, error))?;
        Ok(Delivered::of(passed, result))
    }

    /// Number `number` in the checkout's repository.
    fn object(&self, number: u64) -> Object {
        Object {
            owner: self.owner.clone(),
            repo: self.repo.clone(),
            number,
        }
    }
}

/// The checkout Board was started in, its GitHub repository and its mount
/// in the personal board.
pub(crate) fn origin(host: &dyn Host) -> Result<Origin<'_>, PluginError> {
    origin_at(host, git::top(host)?)
}

/// [`origin`] for the checkout whose root is `top`.
fn origin_at(host: &dyn Host, top: PathBuf) -> Result<Origin<'_>, PluginError> {
    let (owner, repo) = remote::repository(host, &top)?;
    let personal = crossing::personal(host)?;
    let mount = crossing::mount(&*personal, &top)?;
    Ok(Origin {
        host,
        personal,
        top,
        owner,
        repo,
        mount,
    })
}

/// The crossing the boundary passed, and what GitHub returned for what was
/// sent.
///
/// `note` says why nothing was checked, when no boundary covered the
/// content. `sent` is what GitHub returned, also recorded on the crossing.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "SentOutput")]
pub(crate) struct Delivered {
    pub(crate) digest: String,
    pub(crate) crossing: String,
    pub(crate) note: Option<String>,
    pub(crate) sent: String,
}

impl Delivered {
    fn of(passed: Passed, sent: String) -> Self {
        Self {
            digest: passed.digest,
            crossing: passed.crossing,
            note: passed.note,
            sent,
        }
    }
}

/// The branch pushed and the commits sent, oldest first; `sent` is git's
/// line for the branch.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PushOutput")]
pub(crate) struct Pushed {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    branch: String,
    commits: Vec<String>,
}

/// The pushed branch and the pull request opened from it; `sent` is its URL
/// and `path` its record.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrCreateOutput")]
pub(crate) struct Opened {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    branch: String,
    commits: Vec<String>,
    number: u64,
    path: String,
}

/// The issue opened; `sent` is its URL and `path` its record.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "IssueCreateOutput")]
pub(crate) struct Filed {
    digest: String,
    crossing: String,
    note: Option<String>,
    sent: String,
    number: u64,
    path: String,
}

/// What a push left: the check that passed, the branch, the commits and
/// git's line.
struct Push {
    passed: Passed,
    branch: String,
    commits: Vec<String>,
    line: String,
}

/// The checkout Board was started in and the branch it is on, read before
/// the personal board is opened, so a detached HEAD is
/// `github::checkout::detached` whatever the personal board's state.
fn branched(host: &dyn Host) -> Result<(Origin<'_>, String, String), PluginError> {
    let top = git::top(host)?;
    let (branch, head) = git::branch(host, &top)?;
    Ok((origin_at(host, top)?, branch, head))
}

/// Checks the commits a push of `head` to `branch` would send, with `extra`
/// parts, then pushes the commit checked. git's line is recorded at once,
/// so the crossing says the commits left even when a later step fails.
fn pushed(
    place: &Origin<'_>,
    branch: String,
    head: &str,
    action: Pushing,
    extra: Parts,
) -> Result<Push, PluginError> {
    let (host, top) = (place.host, &place.top);
    let have = git::advertised(host, top, "origin")?;
    let (commits, mut parts) = push_parts(host, top, &branch, head, &have)?;
    parts.extend(extra.clone());
    let passed = place.check(action.capability(), &parts)?;
    let handed = Handed {
        digest: passed.digest.clone(),
        action,
        parts: extra,
    };
    let line = git::push(host, top, &branch, head, &handed)?;
    let pushed_already = |error| {
        let lead = match action {
            Pushing::Push => {
                format!("{head} was already pushed to {branch} on origin, so do not push it again")
            }
            Pushing::PrCreate => format!(
                "{head} was pushed to {branch} on origin, but the pull request was not opened; \
                 run pr-create again to open it"
            ),
        };
        after_sent(&lead, &line, error)
    };
    crossing::record(&*place.personal, &passed.digest, &line).map_err(pushed_already)?;
    git::track(host, top, &branch).map_err(pushed_already)?;
    Ok(Push {
        passed,
        branch,
        commits,
        line,
    })
}

/// Push the branch checked out to origin, once the boundary passes the
/// commits it would send.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PushInput")]
pub(crate) struct PushInput {}

/// Pushes the branch checked out, once the boundary passes its name and
/// each commit it would send.
pub(crate) fn push(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PushInput {} = PluginError::read_input("github", "push", input)?;
    let (place, branch, head) = branched(host)?;
    let Push {
        passed,
        branch,
        commits,
        line,
    } = pushed(&place, branch, &head, Pushing::Push, Parts::new())?;
    Ok(output(&Pushed {
        digest: passed.digest,
        crossing: passed.crossing,
        note: passed.note,
        sent: line,
        branch,
        commits,
    }))
}

/// Push the branch checked out and open a pull request from it, once the
/// boundary passes both.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrCreateInput")]
pub(crate) struct PrCreateInput {
    /// The pull request's title.
    #[schemars(length(min = 1))]
    title: String,
    /// The pull request's body, in Markdown.
    body: String,
    /// The branch to merge into; GitHub's default branch when left out.
    #[schemars(length(min = 1))]
    base: Option<String>,
    /// Whether to open it as a draft.
    draft: Option<bool>,
}

/// Pushes the branch and opens a pull request from it, once the boundary
/// passes the commits, title and body.
pub(crate) fn pr_create(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PrCreateInput {
        title,
        body,
        base,
        draft,
    } = PluginError::read_input("github", "pr-create", input)?;
    let (place, branch, head) = branched(host)?;
    let extra = Parts::from([
        ("pr.title".to_owned(), title.clone()),
        ("pr.body".to_owned(), body.clone()),
    ]);
    let Push {
        passed,
        branch,
        commits,
        ..
    } = pushed(&place, branch, &head, Pushing::PrCreate, extra)?;
    let repository = format!("{}/{}", place.owner, place.repo);
    let mut arguments: Vec<String> = ["pr", "create", "--repo", &repository, "--head", &branch]
        .into_iter()
        .chain(["--title", &title, "--body-file", "-"])
        .map(str::to_owned)
        .collect();
    if let Some(base) = base {
        arguments.extend(["--base".to_owned(), base]);
    }
    if draft == Some(true) {
        arguments.push("--draft".to_owned());
    }
    // In the checkout, so gh reads no other repository's state.
    let printed = gh::send(host, &arguments, &body, "gh pr create", Some(&place.top))?;
    let url = printed
        .split_whitespace()
        .last()
        .unwrap_or_default()
        .to_owned();
    let number = (url.rsplit_once("/pull/"))
        .filter(|_| url.starts_with("https://"))
        .and_then(|(_, number)| number.parse::<u64>().ok());
    let Some(number) = number else {
        return Err(gh::unsent(
            "gh pr create",
            &format!("printed no pull request URL after pushing {branch}"),
        ));
    };
    let (path, _) = write(host, &place.object(number), Kind::Pull, &url, Some(&title))
        .map_err(|error| sent_already(&url, error))?;
    let Delivered {
        digest,
        crossing,
        note,
        sent,
    } = place.sent(passed, url)?;
    Ok(output(&Opened {
        digest,
        crossing,
        note,
        sent,
        branch,
        commits,
        number,
        path,
    }))
}

/// Change a pull request's title or body, once the boundary passes the new
/// text.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "PrEditInput")]
#[schemars(extend("anyOf" = [{"required": ["title"]}, {"required": ["body"]}]))]
pub(crate) struct PrEditInput {
    /// The pull request's number in the checkout's repository.
    #[schemars(range(min = 1))]
    number: u64,
    /// The new title.
    #[schemars(length(min = 1))]
    title: Option<String>,
    /// The new body, in Markdown.
    body: Option<String>,
}

/// Changes a pull request's title or body, once the boundary passes the new
/// text.
pub(crate) fn pr_edit(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let PrEditInput {
        number,
        title,
        body,
    } = PluginError::read_input("github", "pr-edit", input)?;
    let place = origin(host)?;
    let mut fields = serde_json::Map::new();
    let mut parts = Parts::new();
    for (name, text) in [("title", title), ("body", body)] {
        if let Some(text) = text {
            parts.insert(format!("pr.{name}"), text.clone());
            fields.insert(name.to_owned(), Value::from(text));
        }
    }
    let passed = place.check("pr-edit", &parts)?;
    let endpoint = format!("repos/{}/{}/pulls/{number}", place.owner, place.repo);
    let (_, url) = gh::send_json(host, "PATCH", &endpoint, &Value::Object(fields))?;
    Ok(output(&place.sent(passed, url)?))
}

/// Comment on a pull request or issue, once the boundary passes the
/// comment.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CommentInput")]
pub(crate) struct CommentInput {
    /// The pull request's or issue's number in the checkout's repository.
    #[schemars(range(min = 1))]
    number: u64,
    /// The comment, in Markdown.
    #[schemars(length(min = 1))]
    body: String,
}

/// Comments on a pull request or issue, once the boundary passes the
/// comment.
pub(crate) fn comment(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let CommentInput { number, body } = PluginError::read_input("github", "comment", input)?;
    let place = origin(host)?;
    let parts = Parts::from([("comment.body".to_owned(), body.clone())]);
    let passed = place.check("comment", &parts)?;
    let endpoint = format!(
        "repos/{}/{}/issues/{number}/comments",
        place.owner, place.repo
    );
    let (_, url) = gh::send_json(host, "POST", &endpoint, &json!({"body": body}))?;
    Ok(output(&place.sent(passed, url)?))
}

/// Open an issue in the checkout's repository, once the boundary passes it.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "IssueCreateInput")]
pub(crate) struct IssueCreateInput {
    /// The issue's title.
    #[schemars(length(min = 1))]
    title: String,
    /// The issue's body, in Markdown.
    body: String,
}

/// Opens an issue, once the boundary passes its title and body, and writes
/// its record.
pub(crate) fn issue_create(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let IssueCreateInput { title, body } =
        PluginError::read_input("github", "issue-create", input)?;
    let place = origin(host)?;
    let parts = Parts::from([
        ("issue.title".to_owned(), title.clone()),
        ("issue.body".to_owned(), body.clone()),
    ]);
    let passed = place.check("issue-create", &parts)?;
    let endpoint = format!("repos/{}/{}/issues", place.owner, place.repo);
    let body = json!({"title": title, "body": body});
    let (answer, url) = gh::send_json(host, "POST", &endpoint, &body)?;
    let Some(number) = answer.get("number").and_then(Value::as_u64) else {
        return Err(gh::unsent(
            "gh api --method POST issues",
            "answered without the issue's number",
        ));
    };
    let (path, _) = write(host, &place.object(number), Kind::Issue, &url, Some(&title))
        .map_err(|error| sent_already(&url, error))?;
    let Delivered {
        digest,
        crossing,
        note,
        sent,
    } = place.sent(passed, url)?;
    Ok(output(&Filed {
        digest,
        crossing,
        note,
        sent,
        number,
        path,
    }))
}

/// `error`, a failure after GitHub took the content at `url`, saying so.
fn sent_already(url: &str, error: PluginError) -> PluginError {
    let lead = format!("{url} was already sent, so do not send it again");
    after_sent(&lead, url, error)
}

/// `error`, a failure after content reached GitHub, starting with `lead`,
/// which says what was sent and whether to run the action again, and
/// naming what GitHub returned, `sent`, in `fields.sent`.
pub(crate) fn after_sent(lead: &str, sent: &str, mut error: PluginError) -> PluginError {
    error.message = format!("{lead}; {}", error.message);
    error.fields.insert("sent".to_owned(), Value::from(sent));
    error
}

/// An output as JSON.
pub(crate) fn output(value: &impl Serialize) -> Value {
    serde_json::to_value(value).expect("an output holds only strings, numbers, lists and maps")
}
