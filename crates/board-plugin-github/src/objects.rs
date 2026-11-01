//! The pull request and issue records: their types, where they live, and
//! finding one already written.

use board_model::parse_json;
use board_ops::{Host, Limit, PluginError};
use board_plugin::shape::Record;
use schemars::JsonSchema;
use serde::Serialize;
use serde_json::{Map, Value};

use crate::records::{IssueRecord, PullRecord};

/// `owner/repo#number`. A repository name may start with a dot, but `.`
/// and `..` are not names.
pub(crate) const REF_PATTERN: &str =
    r"^[A-Za-z0-9][A-Za-z0-9-]*/(\.?[A-Za-z0-9_-][A-Za-z0-9._-]*)#[1-9][0-9]{0,9}$";

/// Which record a pull request or issue has.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
pub(crate) enum Kind {
    #[serde(rename = "github/pull/v1")]
    Pull,
    #[serde(rename = "github/issue/v1")]
    Issue,
}

impl Kind {
    pub(crate) const ALL: [Self; 2] = [Self::Pull, Self::Issue];

    pub(crate) const fn record_type(self) -> &'static str {
        match self {
            Self::Pull => PullRecord::TYPE,
            Self::Issue => IssueRecord::TYPE,
        }
    }

    /// The folder under `/gh/<owner>/<repo>/` that holds the kind, as
    /// GitHub names it in its API.
    const fn folder(self) -> &'static str {
        match self {
            Self::Pull => "pulls",
            Self::Issue => "issues",
        }
    }

    fn of(record_type: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.record_type() == record_type)
    }
}

/// A pull request or issue, `owner/repo#number`, with owner and repository
/// in lower case, since GitHub matches them without case, so one object has
/// one path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Object {
    pub(crate) owner: String,
    pub(crate) repo: String,
    pub(crate) number: u64,
}

impl Object {
    /// The object a reference Board has checked against `REF_PATTERN`
    /// names.
    pub(crate) fn parse(text: &str) -> Option<Self> {
        let text = text.to_lowercase();
        let (owner, rest) = text.split_once('/')?;
        let (repo, digits) = rest.split_once('#')?;
        Some(Self {
            owner: owner.to_owned(),
            repo: repo.to_owned(),
            number: digits.parse().ok()?,
        })
    }

    /// The logical path of its record of `kind`.
    pub(crate) fn path(&self, kind: Kind) -> String {
        let Self {
            owner,
            repo,
            number,
        } = self;
        format!("/gh/{owner}/{repo}/{}/{number}", kind.folder())
    }

    /// GitHub's API endpoint for it, which answers for a pull request too.
    pub(crate) fn endpoint(&self) -> String {
        let Self {
            owner,
            repo,
            number,
        } = self;
        format!("repos/{owner}/{repo}/issues/{number}")
    }
}

/// A record already written: its path, kind and body.
pub(crate) struct Held {
    pub(crate) path: String,
    pub(crate) kind: Kind,
    pub(crate) record: Map<String, Value>,
}

/// The pull or issue record already written for `object`, or none. A record
/// Board marks degraded is `github::record::degraded`, since its fields
/// cannot be trusted.
pub(crate) fn held(host: &dyn Host, object: &Object) -> Result<Option<Held>, PluginError> {
    let paths: Vec<String> = (Kind::ALL.into_iter())
        .map(|kind| literal(&object.path(kind)))
        .collect();
    let kinds: Vec<String> = (Kind::ALL.into_iter())
        .map(|kind| literal(kind.record_type()))
        .collect();
    let sql = format!(
        "SELECT logical, code, body, type FROM records WHERE logical IN ({}) AND type IN ({}) \
         ORDER BY logical",
        paths.join(", "),
        kinds.join(", ")
    );
    let report = (host.query(&sql, Limit::ALL))
        .map_err(|error| PluginError::board_refused("github", "query", &error.refusal()))?;
    let Some([Value::String(path), code, body, Value::String(record_type)]) =
        report.rows.first().map(Vec::as_slice)
    else {
        return Ok(None);
    };
    if let Value::String(code) = code {
        let mut fields = Map::new();
        fields.insert("path".to_owned(), Value::from(path.as_str()));
        fields.insert("code".to_owned(), Value::from(code.as_str()));
        return Err(PluginError {
            code: "github::record::degraded".to_owned(),
            message: format!(
                "the record at {path} is degraded with {code}; run `board check {path}` and fix it"
            ),
            fields,
        });
    }
    let record = (body.as_str())
        .and_then(|body| parse_json(body.as_bytes()).ok())
        .and_then(|record| match record {
            Value::Object(record) => Some(record),
            _ => None,
        });
    match (record, Kind::of(record_type)) {
        (Some(record), Some(kind)) => Ok(Some(Held {
            path: path.clone(),
            kind,
            record,
        })),
        _ => Ok(None),
    }
}

/// `text` as an SQL string literal.
fn literal(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}
