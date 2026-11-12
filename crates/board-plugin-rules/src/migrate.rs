//! `controls.migrate --owed_by true`: sets each obligation's `owed_by` from
//! what its own text and sources name.
//!
//! Three rules are tried in order, and the first that gives exactly one
//! record decides. Rule 1 reads the pull request the requirement names as
//! `owner/repo#n`, or as `#n` after a word such as `PR` or `pull` with a mount
//! in `applies_to` whose clone's origin gives the owner and repository. A
//! number the board holds as an issue record is no match. Rule 2 takes the one
//! `review/report/v1` among `sources`, and rule 3 the one `plans/decision/v1`
//! among them. A pull request the board holds no record of lists the
//! `github.ref` command that writes it, and sets nothing. Two records under
//! one rule are `ambiguous`. Only this board's own controls are read, because
//! a nested board may reference only itself.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

use board_model::parse_json;
use board_ops::{Absent, Bulk, Host, Limit, Origin, PluginError, literal};
use board_validate::Schema;
use regex::Regex;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::logical;
use crate::records::{CONTROL_TYPE, FORMER_SCHEMAS, OWED_BY_TYPES};

/// The reason the migration's commit gives in its `Board-Bulk:` trailer.
const BULK: &str = "migrate controls";
/// The most rewritten paths the output lists (STANDARDS 23).
const LISTED: usize = 100;
const UNREAD: &str = "rules::migrate::unread";
const AMBIGUOUS: &str = "rules::migrate::ambiguous";
const MOUNT_TYPE: &str = "board/mount/v1";
const PULL_TYPE: &str = OWED_BY_TYPES[0];
const REPORT_TYPE: &str = OWED_BY_TYPES[1];
const DECISION_TYPE: &str = OWED_BY_TYPES[2];
/// What `github.ref` writes for a number that is an issue.
const ISSUE_TYPE: &str = "github/issue/v1";

fn compiled(pattern: &str) -> Regex {
    Regex::new(pattern).expect("each pattern is a constant; the migrate tests compile it")
}

/// `owner/repo#n` in prose.
static FULL: LazyLock<Regex> = LazyLock::new(|| {
    compiled(r"(?:^|[^A-Za-z0-9_./-])([A-Za-z0-9][A-Za-z0-9-]*)/([A-Za-z0-9_.-]+)#([0-9]+)\b")
});
/// `#n` after a word that names a pull request, and any more numbers joined to
/// it by `and` or commas. A bare `#n` elsewhere may number anything, such as a
/// finding.
static BARE: LazyLock<Regex> = LazyLock::new(|| {
    compiled(
        r"(?i)\b(?:prs?|pulls?(?:\s+requests?)?)\s+(#[0-9]+(?:(?:\s*,\s*(?:and\s+)?|\s+and\s+)#[0-9]+)*)\b",
    )
});
/// One number in what [`BARE`] captured.
static NUMBER: LazyLock<Regex> = LazyLock::new(|| compiled(r"#([0-9]+)"));
/// The owner and repository of a GitHub remote.
static REMOTE: LazyLock<Regex> =
    LazyLock::new(|| compiled(r"github\.com[:/]([A-Za-z0-9-]+)/([A-Za-z0-9_.-]+?)(?:\.git)?/?$"));

/// Whether to write the migration and which one to run.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ControlsMigrateInput")]
pub(crate) struct Input {
    /// Write every rewritten control; without it nothing is written.
    #[serde(default)]
    apply: bool,
    /// Set each obligation's `owed_by`; without it nothing is read or written.
    #[serde(default)]
    owed_by: bool,
}

/// What the migration set or would set.
///
/// `rewritten` lists each obligation and what it is owed by, at most 100,
/// with `total` and `truncated`. `commands` lists what writes each pull
/// record the board lacks. `ambiguous` lists each obligation two records
/// match under one rule, `unmatched` each that names no record, and `unread`
/// each Board could not read.
#[derive(Debug, Default, Serialize, JsonSchema)]
#[schemars(rename = "ControlsMigrateOutput")]
pub(crate) struct Output {
    applied: bool,
    rewritten: Vec<Rewritten>,
    total: usize,
    truncated: bool,
    commands: Vec<String>,
    ambiguous: Vec<Ambiguous>,
    unmatched: Vec<String>,
    unread: Vec<Unread>,
}

/// An obligation, and what it is owed by.
#[derive(Debug, Serialize, JsonSchema)]
struct Rewritten {
    path: String,
    owed_by: String,
}

/// An obligation that two records match under one rule.
#[derive(Debug, Serialize, JsonSchema)]
struct Ambiguous {
    path: String,
    code: &'static str,
    records: Vec<String>,
}

/// A control left as it is, and why.
#[derive(Debug, Serialize, JsonSchema)]
struct Unread {
    path: String,
    code: &'static str,
    why: String,
}

/// What the rules find for one obligation.
#[derive(Debug, PartialEq)]
enum Found {
    One(String),
    Lacking(String),
    Several(Vec<String>),
    Nothing,
}

/// A pull request as `owner/repo#number`, in the lower case `github.ref`
/// stores it under.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Pull {
    owner: String,
    repo: String,
    number: String,
}

impl Pull {
    fn path(&self) -> String {
        format!("/gh/{}/{}/pulls/{}", self.owner, self.repo, self.number)
    }

    fn issue_path(&self) -> String {
        format!("/gh/{}/{}/issues/{}", self.owner, self.repo, self.number)
    }

    fn command(&self) -> String {
        format!(
            "board call github.ref --ref {}/{}#{}",
            self.owner, self.repo, self.number
        )
    }
}

/// The rows of `sql`, or the refusal of a query Board cut short.
fn rows(host: &dyn Host, sql: &str) -> Result<Vec<Vec<Value>>, PluginError> {
    let report = (host.query(sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("rules", "query", &error.refusal()))?;
    if report.truncated {
        return Err(PluginError {
            code: "rules::migrate::over_budget".to_owned(),
            message: "the board's controls passed the output budget of one board run; migrate a \
                      smaller board, or raise the budget"
                .to_owned(),
            fields: Map::new(),
            hint: None,
        });
    }
    Ok(report.rows)
}

/// The path of each record of the types `owed_by` may name, and of each issue
/// record, in this board's own tree, with its type.
fn kinds(host: &dyn Host) -> Result<BTreeMap<String, String>, PluginError> {
    let types: Vec<String> = (OWED_BY_TYPES.iter().chain([&ISSUE_TYPE]))
        .map(|each| literal(each))
        .collect();
    let sql = format!(
        "SELECT logical, type FROM entries WHERE source = 'worktree' AND type IN ({})",
        types.join(", ")
    );
    Ok((rows(host, &sql)?.iter())
        .filter_map(|row| {
            Some((
                row.first()?.as_str()?.to_owned(),
                row.get(1)?.as_str()?.to_owned(),
            ))
        })
        .collect())
}

/// The owner and repository each of this board's mounts follows, by mount,
/// for those whose clone's origin is on GitHub.
fn mounts(host: &dyn Host) -> Result<BTreeMap<String, (String, String)>, PluginError> {
    let sql = format!(
        "SELECT logical FROM entries WHERE source = 'worktree' AND type = {}",
        literal(MOUNT_TYPE)
    );
    let mounts: Vec<String> = (rows(host, &sql)?.iter())
        .filter_map(|row| Some(row.first()?.as_str()?.to_owned()))
        .collect();
    let asked = (mounts.iter())
        .map(|mount| logical(mount))
        .collect::<Result<Vec<_>, _>>()?;
    if asked.is_empty() {
        return Ok(BTreeMap::new());
    }
    let answers = (host.refs(&asked, Absent::Locate, &[]))
        .map_err(|error| PluginError::board_refused("rules", "refs", &error.refusal()))?;
    let mut found = BTreeMap::new();
    for (mount, answer) in mounts.into_iter().zip(answers) {
        let Ok(report) = answer else {
            continue;
        };
        let Origin::Url(url) = host.clone_origin(&report.location.board.repository) else {
            continue;
        };
        if let Some(captures) = REMOTE.captures(url.trim()) {
            found.insert(
                mount,
                (captures[1].to_lowercase(), captures[2].to_lowercase()),
            );
        }
    }
    Ok(found)
}

/// The pull requests `requirement` names, each once, with a bare `#n` read
/// against each mount `applies_to` reaches.
fn pulls(
    requirement: &str,
    applies_to: &[String],
    mounts: &BTreeMap<String, (String, String)>,
) -> BTreeSet<Pull> {
    let mut found = BTreeSet::new();
    for captures in FULL.captures_iter(requirement) {
        found.insert(Pull {
            owner: captures[1].to_lowercase(),
            repo: captures[2].to_lowercase(),
            number: captures[3].to_owned(),
        });
    }
    let places: BTreeSet<&(String, String)> = (mounts.iter())
        .filter(|(mount, _)| {
            let below = format!("{mount}/");
            (applies_to.iter()).any(|each| each == *mount || each.starts_with(&below))
        })
        .map(|(_, place)| place)
        .collect();
    for list in BARE.captures_iter(requirement) {
        for captures in NUMBER.captures_iter(&list[1]) {
            for (owner, repo) in &places {
                found.insert(Pull {
                    owner: owner.clone(),
                    repo: repo.clone(),
                    number: captures[1].to_owned(),
                });
            }
        }
    }
    found
}

/// What the three rules find for an obligation, in order.
fn find(named: &BTreeSet<Pull>, sources: &[String], kinds: &BTreeMap<String, String>) -> Found {
    // A number the board holds as an issue names no pull request.
    let named: Vec<&Pull> = (named.iter())
        .filter(|pull| kinds.get(&pull.issue_path()).map(String::as_str) != Some(ISSUE_TYPE))
        .collect();
    if named.len() > 1 {
        return Found::Several(named.iter().map(|pull| pull.path()).collect());
    }
    if let Some(pull) = named.first() {
        let path = pull.path();
        return if kinds.get(&path).map(String::as_str) == Some(PULL_TYPE) {
            Found::One(path)
        } else {
            Found::Lacking(pull.command())
        };
    }
    for kind in [REPORT_TYPE, DECISION_TYPE] {
        let of: Vec<&String> = (sources.iter())
            .filter(|each| kinds.get(*each).map(String::as_str) == Some(kind))
            .collect();
        match of[..] {
            [] => {}
            [one] => return Found::One(one.clone()),
            _ => return Found::Several(of.into_iter().cloned().collect()),
        }
    }
    Found::Nothing
}

/// The strings of the list `key` of `body`.
fn strings(body: &Map<String, Value>, key: &str) -> Vec<String> {
    (body
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten())
    .filter_map(|each| each.as_str().map(str::to_owned))
    .collect()
}

pub(crate) fn migrate(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input { apply, owed_by } = PluginError::read_input("rules", "migrate", input)?;
    let mut output = Output {
        applied: apply,
        ..Output::default()
    };
    if owed_by {
        let lines = owed_by_lines(host, &mut output)?;
        if apply && !lines.is_empty() {
            let refused = |refusal: &board_model::Refusal| {
                PluginError::board_refused("rules", "put --batch --bulk", refusal)
            };
            let bulk = Bulk::new(BULK).map_err(|error| refused(&error.refusal()))?;
            (host.put_bulk(&lines, &bulk)).map_err(|error| refused(&error.refusal()))?;
        }
    }
    output.total = output.rewritten.len();
    output.truncated = output.total > LISTED;
    output.rewritten.truncate(LISTED);
    Ok(serde_json::to_value(output).expect("Output holds only strings, numbers and bools"))
}

/// The lines that set `owed_by` on each obligation that lacks it, with
/// `output` filled in.
fn owed_by_lines(host: &dyn Host, output: &mut Output) -> Result<Vec<Value>, PluginError> {
    // Only rules of the type before the rename predate owed_by, so only they
    // are read, against that type's schema.
    let schema = serde_json::from_str(FORMER_SCHEMAS[0]).map(Schema::compile);
    let schema = (schema.ok().and_then(Result::ok))
        .expect("the former rule schema is committed JSON; a test compiles it");
    let kinds = kinds(host)?;
    let mounts = mounts(host)?;
    let sql = format!(
        "SELECT e.logical, e.blob, b.body FROM entries AS e JOIN bodies AS b ON b.blob = e.blob \
         WHERE e.type = {} AND e.source = 'worktree' ORDER BY e.logical",
        literal(CONTROL_TYPE)
    );
    let mut lines = Vec::new();
    for row in rows(host, &sql)? {
        let [
            Value::String(path),
            Value::String(blob),
            Value::String(text),
        ] = &row[..]
        else {
            continue;
        };
        let unread = |why: String| Unread {
            path: path.clone(),
            code: UNREAD,
            why,
        };
        let Ok(Value::Object(mut body)) = parse_json(text.as_bytes()) else {
            output
                .unread
                .push(unread("it does not read as a JSON object".to_owned()));
            continue;
        };
        if body.get("kind").and_then(Value::as_str) != Some("obligation")
            || body.get("owed_by").is_some_and(|held| !held.is_null())
        {
            continue;
        }
        let Some(requirement) = body.get("requirement").and_then(Value::as_str) else {
            output.unread.push(unread(
                "its requirement is no text; fix the control".to_owned(),
            ));
            continue;
        };
        let named = pulls(requirement, &strings(&body, "applies_to"), &mounts);
        match find(&named, &strings(&body, "sources"), &kinds) {
            Found::One(owed) => {
                body.insert("owed_by".to_owned(), json!(owed));
                let record = Value::Object(body);
                if let Err(error) = schema.validate(&record) {
                    output.unread.push(unread(format!(
                        "its rewritten record {error}; fix the control and run migrate again"
                    )));
                    continue;
                }
                output.rewritten.push(Rewritten {
                    path: path.clone(),
                    owed_by: owed,
                });
                lines.push(json!({"path": path, "expect": blob, "record": record}));
            }
            Found::Lacking(command) => {
                if !output.commands.contains(&command) {
                    output.commands.push(command);
                }
            }
            Found::Several(records) => output.ambiguous.push(Ambiguous {
                path: path.clone(),
                code: AMBIGUOUS,
                records,
            }),
            Found::Nothing => output.unmatched.push(path.clone()),
        }
    }
    Ok(lines)
}
