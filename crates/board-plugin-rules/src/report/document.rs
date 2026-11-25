//! The `rules/report/v1` document `report` prints: its record type, whose
//! schema is generated from these structs, and the parts it holds.

use std::collections::BTreeMap;

use board_plugin::shape::{
    CodeName, CommitName, DottedVersion, ObjectId, PathKey, Prose, Record, Remote, Repository,
    Summary, Tag, Timestamp,
};
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::Serialize;
use serde_json::Value;

use crate::records::{Checker, Kind, ShowAt, rule_output_schema};

/// `rules/report/v1`: every rule, group and compliance suite that reaches
/// one repository, what each found, where the repository's own rules differ
/// from the imposed set, and every refusal met while reading them.
///
/// `board rules report` prints one and writes nothing. File paths are from
/// the repository's root, so reports from different boards agree. A reader
/// reads `result`, a group's `state` and a field list through `Open`, so a
/// value a later Board adds reads as unknown.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReportRecord {
    /// The record's type.
    #[serde(rename = "$type")]
    pub(crate) tag: Tag<Self>,
    /// When the command ran.
    #[schemars(with = "Timestamp")]
    pub(crate) generated: String,
    /// The repository the report measured.
    pub(crate) repository: RepositoryRead,
    /// The Board that made the report and the board its rules came from.
    pub(crate) board: BoardRead,
    /// The GitHub Actions run that made the report, from its variables;
    /// null outside GitHub Actions or when a variable is missing or
    /// malformed.
    pub(crate) source_run: Option<SourceRun>,
    /// The imposed rule set.
    pub(crate) imposed: Imposed,
    /// Each rule that reaches the repository, the imposed set's first by
    /// path, then the repository's own.
    pub(crate) rules: Vec<Row>,
    /// Each group and the state its members roll up to.
    pub(crate) groups: Vec<GroupRow>,
    /// Each compliance suite whose `subjects` name the repository, and its
    /// newest full run of a clean revision.
    pub(crate) suites: Vec<SuiteRow>,
    /// Where the repository's own rules differ from the imposed set.
    pub(crate) drift: Drift,
    /// Each refusal and warning met while reading, which the rows it names
    /// also show.
    pub(crate) refused: Vec<Refused>,
}

impl Record for ReportRecord {
    const TYPE: &'static str = "rules/report/v1";
}

/// The repository a report measured.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RepositoryRead {
    /// The repository's name: its origin remote in identity form, or what
    /// `--repository` gave.
    #[schemars(with = "Remote")]
    pub(crate) name: String,
    /// The commit the report read; null when Board read the board's own
    /// tree and git gave no HEAD.
    #[schemars(schema_with = "or_null::<ObjectId>")]
    pub(crate) commit: Option<String>,
    /// When that commit was made; null when git cannot read it.
    #[schemars(schema_with = "or_null::<Timestamp>")]
    pub(crate) committed: Option<String>,
    /// Whether a file git tracks in the checkout read differs from the
    /// commit.
    pub(crate) uncommitted: bool,
}

/// The Board that made a report and the board holding its rules.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BoardRead {
    /// Board's version, such as `0.1.1`.
    #[schemars(with = "DottedVersion")]
    pub(crate) version: String,
    /// The rules board's origin remote in identity form; null when it has
    /// none.
    #[schemars(schema_with = "or_null::<Remote>")]
    pub(crate) rules_from: Option<String>,
    /// The commit HEAD names in the rules board; null when git gives none.
    #[schemars(schema_with = "or_null::<ObjectId>")]
    pub(crate) rules_commit: Option<String>,
}

/// The GitHub Actions run that made a report.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceRun {
    /// `GITHUB_REPOSITORY`: the repository whose workflow ran.
    #[schemars(with = "Repository")]
    pub(crate) repository: String,
    /// `GITHUB_RUN_ID`: the run's id.
    pub(crate) run_id: u64,
    /// `GITHUB_RUN_NUMBER`: the run's number in its workflow.
    pub(crate) run_number: u64,
    /// `GITHUB_SHA`: the commit the run checked out.
    #[schemars(with = "ObjectId")]
    pub(crate) commit: String,
}

/// The imposed rule set.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Imposed {
    /// The set's root, such as `/rules`.
    #[schemars(with = "PathKey")]
    pub(crate) root: String,
}

// Where a rule came from. No doc comment, which would describe the inlined
// enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Source {
    Imposed,
    Repository,
}

// What a rule found. No doc comment, which would describe the inlined enum
// in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Ord, PartialOrd, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub(crate) enum Outcome {
    Within,
    Over,
    Behind,
    Zero,
    NotZero,
    NoRun,
    Stale,
    MissingMeasure,
    Written,
    Unmeasured,
    Unreadable,
}

impl Outcome {
    /// Whether a rule with this result fails.
    pub(crate) fn fails(self) -> bool {
        matches!(
            self,
            Self::Over
                | Self::Behind
                | Self::NotZero
                | Self::NoRun
                | Self::Stale
                | Self::MissingMeasure
        )
    }

    /// Whether a rule with this result was measured and passed.
    pub(crate) fn passes(self) -> bool {
        matches!(self, Self::Within | Self::Zero)
    }
}

/// One rule that reaches the repository and what it found.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Row {
    /// The rule's path: in the rules board for an imposed rule, and from
    /// the repository's root for one of its own.
    #[schemars(with = "PathKey")]
    pub(crate) id: String,
    /// `imposed` for a rule of the imposed set, `repository` for one the
    /// repository holds itself.
    pub(crate) source: Source,
    /// The paths of the groups the rule is in.
    #[schemars(with = "Vec<PathKey>")]
    pub(crate) groups: Vec<String>,
    /// What the files must do, as the rule holds it; null when the rule
    /// cannot be read.
    #[schemars(schema_with = "or_null::<Prose>")]
    pub(crate) requirement: Option<String>,
    /// Why the requirement exists, as the rule holds it; null when the rule
    /// cannot be read.
    #[schemars(schema_with = "or_null::<Prose>")]
    pub(crate) purpose: Option<String>,
    /// `standing` or `obligation`, as the rule holds it.
    pub(crate) kind: Kind,
    /// When the rule is shown, its default applied.
    pub(crate) show: ShowAt,
    /// What a program checks, as the rule holds it; null for a rule with
    /// none, one Board cannot read, or one of a kind this Board does not
    /// know. A pattern checker names its file and never its expressions.
    #[schemars(schema_with = "rule_output_schema")]
    pub(crate) checker: Option<Checker>,
    /// What the rule found: `within` or `over` a budget, a pin `behind`, a
    /// zero rule's state, `written` for a rule this command does not
    /// measure, `unmeasured` for the repository's own rule, or
    /// `unreadable` with `code`.
    pub(crate) result: Outcome,
    /// Why the result is `unreadable`; null otherwise.
    #[schemars(schema_with = "or_null::<CodeName>")]
    pub(crate) code: Option<String>,
    /// What a budget's files use, its largest file's for a per-file budget;
    /// null for any other rule.
    pub(crate) used: Option<u64>,
    /// A budget's max; null for any other rule.
    pub(crate) max: Option<u64>,
    /// The files a budget or version rule read, from the repository's root,
    /// at most 20.
    #[schemars(with = "Vec<PathKey>")]
    pub(crate) files: Vec<String>,
    /// Each file over a per-file budget, at most 20.
    pub(crate) over: Vec<FileOver>,
    /// Each pin older than a version rule's `at_least`, at most 20.
    pub(crate) behind: Vec<PinBehind>,
    /// How many items each list above left out.
    pub(crate) omitted: Omitted,
    /// What a zero rule's suite's latest run reported; null for any other
    /// rule.
    pub(crate) zero: Option<ZeroRead>,
}

/// A file over a per-file budget and what it uses.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FileOver {
    /// The file, from the repository's root.
    #[schemars(with = "PathKey")]
    pub(crate) file: String,
    /// What it uses.
    pub(crate) used: u64,
}

/// A pin older than a version rule's floor.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PinBehind {
    /// The file, from the repository's root.
    #[schemars(with = "PathKey")]
    pub(crate) file: String,
    /// The line the pin is on, from 1.
    pub(crate) line: u64,
    /// The version pinned.
    #[schemars(with = "DottedVersion")]
    pub(crate) found: String,
}

/// How many items a row's lists left out past the first 20.
#[derive(Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Omitted {
    /// Files left out of `files`.
    pub(crate) files: u64,
    /// Files left out of `over`.
    pub(crate) over: u64,
    /// Pins left out of `behind`.
    pub(crate) behind: u64,
}

/// What a zero rule's suite's latest qualifying run reported.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ZeroRead {
    /// The suite, by its logical path.
    #[schemars(with = "PathKey")]
    pub(crate) suite: String,
    /// The run, by its logical path; null when none qualifies.
    #[schemars(schema_with = "or_null::<PathKey>")]
    pub(crate) run: Option<String>,
    /// Whole seconds since the run finished; null with no run.
    pub(crate) age_s: Option<u64>,
    /// Each measure the rule names and the value the run recorded, a
    /// number, a label or a flag, null when it recorded none.
    #[schemars(schema_with = "measures_or_null")]
    pub(crate) measures: BTreeMap<String, Option<Value>>,
}

/// A group and the state its members roll up to.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GroupRow {
    /// The group's path.
    #[schemars(with = "PathKey")]
    pub(crate) id: String,
    /// The group's name for people.
    #[schemars(with = "Summary")]
    pub(crate) title: String,
    /// `failing` when a member fails, `unreadable` when one is unreadable
    /// and none fails, `passing` when every measured member passes,
    /// `written` when no member is measured, and `not_reached` when no
    /// member reaches the repository.
    pub(crate) state: GroupState,
    /// How many of the group's members that reach the repository found
    /// each result.
    pub(crate) counts: Counts,
}

// A group's rolled-up state. No doc comment, which would describe the
// inlined enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub(crate) enum GroupState {
    Failing,
    Unreadable,
    Passing,
    Written,
    NotReached,
}

/// How many members found each result.
#[derive(Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Counts {
    /// Budgets within their max.
    pub(crate) within: u64,
    /// Budgets over their max.
    pub(crate) over: u64,
    /// Version rules with a pin behind.
    pub(crate) behind: u64,
    /// Zero rules whose measures are all 0.
    pub(crate) zero: u64,
    /// Zero rules with a measure not 0.
    pub(crate) not_zero: u64,
    /// Zero rules whose suite has no qualifying run.
    pub(crate) no_run: u64,
    /// Zero rules whose run is older than their `within_days`.
    pub(crate) stale: u64,
    /// Zero rules whose run lacks a measure.
    pub(crate) missing_measure: u64,
    /// Rules this command does not measure.
    pub(crate) written: u64,
    /// The repository's own rules.
    pub(crate) unmeasured: u64,
    /// Rules that cannot be read.
    pub(crate) unreadable: u64,
}

impl Counts {
    pub(crate) fn add(&mut self, result: Outcome) {
        let count = match result {
            Outcome::Within => &mut self.within,
            Outcome::Over => &mut self.over,
            Outcome::Behind => &mut self.behind,
            Outcome::Zero => &mut self.zero,
            Outcome::NotZero => &mut self.not_zero,
            Outcome::NoRun => &mut self.no_run,
            Outcome::Stale => &mut self.stale,
            Outcome::MissingMeasure => &mut self.missing_measure,
            Outcome::Written => &mut self.written,
            Outcome::Unmeasured => &mut self.unmeasured,
            Outcome::Unreadable => &mut self.unreadable,
        };
        *count += 1;
    }
}

/// A compliance suite and its newest full run of a clean revision.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuiteRow {
    /// The suite, by its logical path.
    #[schemars(with = "PathKey")]
    pub(crate) id: String,
    /// How many cases the suite lists inline; null when it lists none.
    pub(crate) cases: Option<u64>,
    /// The newest qualifying run; null when none qualifies or `runs.latest`
    /// refused.
    pub(crate) run: Option<SuiteRun>,
    /// Why `runs.latest` refused; null when it answered.
    #[schemars(schema_with = "or_null::<CodeName>")]
    pub(crate) code: Option<String>,
}

/// A suite's newest qualifying run.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SuiteRun {
    /// The run, by its logical path.
    #[schemars(with = "PathKey")]
    pub(crate) id: String,
    /// When the run finished, or started when it records no finish; null
    /// when it records neither as a time.
    #[schemars(schema_with = "or_null::<Timestamp>")]
    pub(crate) finished: Option<String>,
    /// The commit the run's `versions.commit` names; null when it names
    /// none.
    #[schemars(schema_with = "or_null::<CommitName>")]
    pub(crate) commit: Option<String>,
    /// `fail` when a case failed or erred, else `pass`.
    pub(crate) outcome: Verdict,
    /// Each case that failed or erred, with the run's note on it.
    pub(crate) failed: Vec<FailedCase>,
    /// Commits reachable from `repository.commit` and not from the run's
    /// commit, as `git rev-list --count` counts them; null when they
    /// cannot be counted.
    pub(crate) commits_since: Option<u64>,
    /// Why `commits_since` is null: `rules::report::run_commit_unnamed`
    /// when the run names no commit, `rules::report::run_commit_missing`
    /// when the clone lacks it, `rules::report::run_commit_not_ancestor`
    /// or `rules::report::no_git`; null when it is counted.
    #[schemars(schema_with = "or_null::<CodeName>")]
    pub(crate) commits_since_code: Option<String>,
}

// Whether a run passed. No doc comment, which would describe the inlined
// enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
#[schemars(inline)]
pub(crate) enum Verdict {
    Pass,
    Fail,
}

/// A case that failed or erred in a run.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FailedCase {
    /// The case's id, as the suite names it.
    pub(crate) case: String,
    /// The run's note on the case; null when it has none.
    #[schemars(schema_with = "or_null::<Prose>")]
    pub(crate) note: Option<String>,
}

/// Where the repository's own rules differ from the imposed set, comparing
/// rules by their paths below the imposed root.
#[derive(Default, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Drift {
    /// The repository's own rules with no imposed rule of the same path.
    #[schemars(with = "Vec<PathKey>")]
    pub(crate) extra: Vec<String>,
    /// The imposed rules the repository also holds with different fields.
    pub(crate) differs: Vec<Differs>,
    /// The imposed rules that reach no file in the repository.
    #[schemars(with = "Vec<PathKey>")]
    pub(crate) not_reached: Vec<String>,
}

/// An imposed rule the repository also holds, and the fields that differ.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Differs {
    /// The imposed rule's path.
    #[schemars(with = "PathKey")]
    pub(crate) id: String,
    /// The fields whose values differ.
    pub(crate) fields: Vec<Field>,
}

// A rule field drift compares. No doc comment, which would describe the
// inlined enum in the committed schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(inline)]
pub(crate) enum Field {
    Requirement,
    Purpose,
    Summary,
    Checker,
    Kind,
    Show,
}

/// A refusal or warning met while reading.
#[derive(Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Refused {
    /// Board's or the plugin's code.
    #[schemars(with = "CodeName")]
    pub(crate) code: String,
    /// The rules or groups it concerns, by path; empty when it concerns
    /// none.
    #[schemars(with = "Vec<PathKey>")]
    pub(crate) rules: Vec<String>,
    /// What happened, for people.
    #[schemars(with = "Prose")]
    pub(crate) message: String,
}

/// `T` or null, as an `anyOf`, so a shared string shape stays whole.
pub(crate) fn or_null<T: JsonSchema>(generator: &mut SchemaGenerator) -> Schema {
    let shape = generator.subschema_for::<T>();
    json_schema!({"anyOf": [shape, {"type": "null"}]})
}

/// A map of measure names to a number, a label, a flag or null.
fn measures_or_null(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "type": "object",
        "additionalProperties": {"type": ["number", "string", "boolean", "null"]}
    })
}
