//! `report`: one `rules/report/v1` document of every rule, group and
//! compliance suite that reaches a repository, what each found, where the
//! repository's own rules differ from the imposed set, and every refusal.
//! It writes nothing; a repository's CI uploads what it prints.

use std::collections::{BTreeMap, BTreeSet};

use board_model::{LogicalPath, parse_json};
use board_ops::{Host, PluginError, literal};
use board_plugin::shape::Tag;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::check::{self, under};
use crate::glob::glob;
use crate::index::{self, Control};
use crate::records::{Checker, GroupRecord, Kind, RULE_TYPES, ShowAt};
use crate::version;
use crate::zero;

/// The most items a row's `files`, `over` and `behind` list; `omitted`
/// counts the rest.
pub(crate) const LISTED: usize = 20;

/// The imposed set's root when `--imposed` is left out.
const IMPOSED: &str = "/rules";

/// The variables a GitHub Actions run sets, which `source_run` reads.
pub(crate) const CI_VARIABLES: [&str; 4] = [
    "GITHUB_REPOSITORY",
    "GITHUB_RUN_ID",
    "GITHUB_RUN_NUMBER",
    "GITHUB_SHA",
];

/// One report of the rules, groups and compliance suites reaching a
/// repository.
#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "ReportInput")]
pub(crate) struct Input {
    /// The repository to report on, by its logical path, such as /repos/app, or / when the repository is the board itself; / when left out.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    path: Option<String>,
    /// The repository's name, such as owner/app, for a clone with no origin remote; left out, the clone's origin names it.
    repository: Option<String>,
    /// The root of the imposed rule set, such as /rules/imposed; /rules when left out.
    #[schemars(regex(pattern = "^/"), extend("format" = "board-path"))]
    imposed: Option<String>,
}

/// The report on the repository at `path`, `/` when left out.
pub(crate) fn report(host: &dyn Host, input: &Value) -> Result<Value, PluginError> {
    let Input {
        path,
        repository,
        imposed,
    } = PluginError::read_input("rules", "report", input)?;
    let scope = check::scope(host, path.as_deref().unwrap_or("/"))?;
    let imposed = imposed.as_deref().unwrap_or(IMPOSED);
    let imposed: LogicalPath = crate::logical(imposed)?;
    let place = Place::of(host, &scope)?;
    let name = place.name(host, repository.as_deref())?;
    let mut refused = Vec::new();
    let source_run = source_run(host, &mut refused);
    let board = place.board_read(host);
    let commit = place.commit(host);
    let repository = RepositoryRead {
        name,
        committed: commit
            .as_ref()
            .and_then(|commit| place.committed(host, commit)),
        uncommitted: place.uncommitted(host),
        commit,
    };
    let (mut rules, imposed_set) = imposed_rows(host, &scope, &place, &imposed, &mut refused)?;
    let own = own_rows(host, &place)?;
    let drift = drift(&imposed_set, &imposed, &own);
    rules.extend(own.into_iter().map(|(row, _)| row));
    let groups = groups(host, &mut rules, &imposed_set, &mut refused)?;
    let suites = suites(host, &place, &repository, &mut refused)?;
    let report = ReportRecord {
        tag: Tag::default(),
        generated: board_model::utc_seconds(host.settings().now),
        repository,
        board,
        source_run,
        imposed: Imposed {
            root: imposed.to_string(),
        },
        rules,
        groups,
        suites,
        drift,
        refused,
    };
    Ok(serde_json::to_value(report).expect("a report holds only strings, numbers and lists"))
}

/// The GitHub Actions run named by its four variables, or null when
/// `GITHUB_RUN_ID` is unset. With it set and another missing or malformed,
/// null and a `rules::report::ci_env_invalid` refusal.
fn source_run(host: &dyn Host, refused: &mut Vec<Refused>) -> Option<SourceRun> {
    let environment = &host.settings().environment;
    let read = |name: &str| (environment.get(name)).map(|value| value.to_string_lossy());
    read("GITHUB_RUN_ID")?;
    let [repository, run_id, run_number, commit] = CI_VARIABLES.map(read);
    let number = |text: Option<&str>| text.and_then(|text| text.parse::<u64>().ok());
    let repository = repository.filter(|text| is_repository(text));
    let commit = commit.filter(|text| {
        board_model::ObjectId::from_hex(text).is_some() && *text == text.to_lowercase()
    });
    let found = (
        repository,
        number(run_id.as_deref()),
        number(run_number.as_deref()),
        commit,
    );
    if let (Some(repository), Some(run_id), Some(run_number), Some(commit)) = found {
        return Some(SourceRun {
            repository: repository.into_owned(),
            run_id,
            run_number,
            commit: commit.into_owned(),
        });
    }
    refused.push(Refused {
        code: "rules::report::ci_env_invalid".to_owned(),
        rules: Vec::new(),
        message: "GITHUB_RUN_ID is set, but GITHUB_REPOSITORY (owner/name), GITHUB_RUN_ID or \
                  GITHUB_RUN_NUMBER (whole numbers) or GITHUB_SHA (a full commit id) is missing \
                  or malformed, so source_run is null"
            .to_owned(),
    });
    None
}

/// Whether `text` is a repository as GitHub names it, `owner/name`.
fn is_repository(text: &str) -> bool {
    let Some((owner, name)) = text.split_once('/') else {
        return false;
    };
    let owner_ok = !owner.is_empty()
        && (owner.bytes()).all(|byte| byte.is_ascii_alphanumeric() || byte == b'-');
    let name_ok = !name.is_empty()
        && (name.bytes())
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'));
    owner_ok && name_ok
}

/// An imposed rule's facts drift compares, by its path below the imposed
/// root, and whether it reaches a file in the repository.
pub(crate) struct ImposedRule {
    id: String,
    fields: Option<Compared>,
    reached: bool,
}

/// The rule fields drift compares.
#[derive(PartialEq)]
struct Compared {
    requirement: String,
    purpose: String,
    summary: Option<String>,
    checker: Option<Value>,
    kind: Kind,
    show: ShowAt,
}

impl Compared {
    fn of(control: &Control) -> Self {
        Self {
            requirement: control.requirement.clone(),
            purpose: control.purpose.clone(),
            summary: control.summary.clone(),
            checker: (control.checker.as_ref())
                .map(|checker| serde_json::to_value(checker).expect("a checker serializes")),
            kind: control.kind(),
            show: control.show(),
        }
    }

    /// The fields whose values differ from `other`'s.
    fn differ(&self, other: &Self) -> Vec<Field> {
        [
            (self.requirement != other.requirement, Field::Requirement),
            (self.purpose != other.purpose, Field::Purpose),
            (self.summary != other.summary, Field::Summary),
            (self.checker != other.checker, Field::Checker),
            (self.kind != other.kind, Field::Kind),
            (self.show != other.show, Field::Show),
        ]
        .into_iter()
        .filter_map(|(differs, field)| differs.then_some(field))
        .collect()
    }
}

/// The imposed rules' rows, each that reaches the repository measured as
/// `check` measures it, and the imposed set by path below its root.
fn imposed_rows(
    host: &dyn Host,
    scope: &LogicalPath,
    place: &Place,
    root: &LogicalPath,
    refused: &mut Vec<Refused>,
) -> Result<(Vec<Row>, BTreeMap<String, ImposedRule>), PluginError> {
    let in_set = |logical: &str| logical.parse().is_ok_and(|path| under(&path, root));
    let mut measured = index::controls(host)?;
    measured.retain(|logical, _| in_set(logical));
    let unreadable = unreadable_rules(host, &measured, &in_set)?;
    let mut kept = BTreeMap::new();
    for logical in unreadable.keys() {
        if let Some(control) = measured.remove(logical) {
            kept.insert(logical.clone(), control);
        }
    }
    let reached = check::reach(host, scope, &measured)?;
    let touches_a_file =
        |logical: &str| !reached.files[logical].is_empty() || reached.unnamed.contains_key(logical);
    // Under the root `check` lists every rule; the report lists only those that reach a file.
    let applying: Vec<&String> = (reached.applying.iter())
        .filter(|logical| touches_a_file(logical))
        .collect();
    let (budgets, versions) = check::measure_files(
        host,
        &applying,
        &measured,
        &reached.files,
        &reached.unreadable,
    )?;
    let mut rows = Vec::new();
    for budget in budgets {
        rows.push(budget_row(place, &measured[&budget.rule], &budget, refused));
    }
    for pinned in versions {
        rows.push(version_row(
            place,
            &measured[&pinned.rule],
            &pinned,
            refused,
        ));
    }
    for zeroed in zero::zeroed(host, &applying, &measured) {
        rows.push(zero_row(&measured[&zeroed.rule], zeroed, refused));
    }
    let shown: BTreeSet<String> = rows.iter().map(|row| row.id.clone()).collect();
    for logical in applying {
        if !shown.contains(logical) {
            rows.push(Row::of(logical, Source::Imposed, Some(&measured[logical])));
        }
    }
    for (logical, (code, message)) in &unreadable {
        let mut row = Row::of(logical, Source::Imposed, kept.get(logical));
        row.result = Outcome::Unreadable;
        row.code = Some(code.clone());
        refused.push(Refused {
            code: code.clone(),
            rules: vec![logical.clone()],
            message: message.clone(),
        });
        rows.push(row);
    }
    rows.sort_by(|one, other| one.id.cmp(&other.id));
    let key = |logical: &str| below(logical, root.as_str());
    let mut set: BTreeMap<String, ImposedRule> = BTreeMap::new();
    for (logical, control) in &measured {
        let reached = touches_a_file(logical);
        let fields = Some(Compared::of(control));
        let id = logical.clone();
        set.insert(
            key(logical),
            ImposedRule {
                id,
                fields,
                reached,
            },
        );
    }
    for logical in unreadable.keys() {
        let fields = kept.get(logical).map(Compared::of);
        let id = logical.clone();
        set.insert(
            key(logical),
            ImposedRule {
                id,
                fields,
                reached: true,
            },
        );
    }
    Ok((rows, set))
}

/// The imposed rules `report` cannot measure, each with its code and why:
/// those Board marks degraded, and those whose version capture is invalid.
fn unreadable_rules(
    host: &dyn Host,
    controls: &BTreeMap<String, Control>,
    in_set: &dyn Fn(&str) -> bool,
) -> Result<BTreeMap<String, (String, String)>, PluginError> {
    let mut unreadable: BTreeMap<String, (String, String)> = (index::degraded(host)?.into_iter())
        .filter(|(logical, _)| in_set(logical))
        .map(|(logical, code)| {
            let code = code.as_str().unwrap_or("rules::rule::degraded").to_owned();
            let message = format!(
                "Board marks the rule {logical} degraded with {code}; run `board check {logical}` for why"
            );
            (logical, (code, message))
        })
        .collect();
    for (logical, control) in controls {
        if let Some(Checker::Version(rule)) = &control.checker
            && let Err(error) = version::compile(logical, rule)
        {
            unreadable.insert(logical.clone(), (error.code, error.message));
        }
    }
    Ok(unreadable)
}

/// A budget's row: `within` or `over`, or `unreadable` when a file it
/// reaches cannot be read.
fn budget_row(
    place: &Place,
    control: &Control,
    budget: &check::Checked,
    refused: &mut Vec<Refused>,
) -> Row {
    let mut row = Row::of(&budget.rule, Source::Imposed, Some(control));
    row.used = Some(budget.used);
    row.max = Some(budget.max);
    row.result = if budget.used > budget.max {
        Outcome::Over
    } else {
        Outcome::Within
    };
    let over: Vec<FileOver> = (budget.each.iter())
        .flat_map(|each| &each.over)
        .map(|file| FileOver {
            file: place.relative(&file.file),
            used: file.used,
        })
        .collect();
    (row.over, row.omitted.over) = cut(over);
    (row.files, row.omitted.files) = cut(place.all_relative(&budget.files));
    unreadable_row(&mut row, &budget.unreadable, refused);
    row
}

/// A version rule's row: `within` or `behind`, or `unreadable` when a file
/// it reaches cannot be read.
fn version_row(
    place: &Place,
    control: &Control,
    pinned: &check::Pinned,
    refused: &mut Vec<Refused>,
) -> Row {
    let mut row = Row::of(&pinned.rule, Source::Imposed, Some(control));
    row.result = if pinned.behind.is_empty() {
        Outcome::Within
    } else {
        Outcome::Behind
    };
    let behind: Vec<PinBehind> = (pinned.behind.iter())
        .map(|pin| PinBehind {
            file: place.relative(&pin.file),
            line: u64::try_from(pin.line).expect("a line number fits in u64"),
            found: pin.found.clone(),
        })
        .collect();
    (row.behind, row.omitted.behind) = cut(behind);
    (row.files, row.omitted.files) = cut(place.all_relative(&pinned.files));
    unreadable_row(&mut row, &pinned.unreadable, refused);
    row
}

/// A zero rule's row: its state, and what its suite's latest run reported.
fn zero_row(control: &Control, zeroed: zero::Zeroed, refused: &mut Vec<Refused>) -> Row {
    let mut row = Row::of(&zeroed.rule, Source::Imposed, Some(control));
    row.result = match zeroed.state {
        zero::State::Zero => Outcome::Zero,
        zero::State::NotZero => Outcome::NotZero,
        zero::State::NoRun => Outcome::NoRun,
        zero::State::Stale => Outcome::Stale,
        zero::State::MissingMeasure => Outcome::MissingMeasure,
        zero::State::Unreadable => Outcome::Unreadable,
    };
    row.code.clone_from(&zeroed.code);
    if let Some(code) = &zeroed.code {
        refused.push(Refused {
            code: code.clone(),
            rules: vec![zeroed.rule.clone()],
            message: format!(
                "the zero rule {} could not read the latest run of {}",
                zeroed.rule, zeroed.suite
            ),
        });
    }
    row.zero = Some(ZeroRead {
        measures: (zeroed.measures.iter())
            .map(|each| {
                (
                    each.name.clone(),
                    (each.value.clone()).filter(|value| !value.is_null()),
                )
            })
            .collect(),
        suite: zeroed.suite,
        run: zeroed.run,
        age_s: zeroed.age_s,
    });
    row
}

/// A row that read a file it could not: `unreadable` with the first file's
/// code, and a refusal naming each.
fn unreadable_row(row: &mut Row, files: &[check::Unreadable], refused: &mut Vec<Refused>) {
    let Some(first) = files.first() else {
        return;
    };
    let code = (first.code.clone()).unwrap_or_else(|| "rules::report::file_unreadable".to_owned());
    row.result = Outcome::Unreadable;
    row.code = Some(code.clone());
    let named: Vec<&str> = files.iter().map(|file| file.file.as_str()).collect();
    refused.push(Refused {
        code,
        rules: vec![row.id.clone()],
        message: format!(
            "Board cannot read {}, which {} reaches",
            named.join(", "),
            row.id
        ),
    });
}

/// `items` cut to the first [`LISTED`], and how many were left out.
fn cut<T>(mut items: Vec<T>) -> (Vec<T>, u64) {
    let left = items.len().saturating_sub(LISTED);
    items.truncate(LISTED);
    (items, u64::try_from(left).expect("a count fits in u64"))
}

/// `logical` below `root`, such as `rust/version` for `/rules/rust/version`
/// below `/rules`; `logical` itself when it is not below `root`.
fn below(logical: &str, root: &str) -> String {
    let root = root.trim_end_matches('/');
    match logical.strip_prefix(root) {
        Some(rest) if rest.starts_with('/') => rest[1..].to_owned(),
        _ => logical.to_owned(),
    }
}

impl Row {
    /// A row for the rule at `id` with `control`'s fields, when it was
    /// read, and no facts yet.
    fn of(id: &str, source: Source, control: Option<&Control>) -> Self {
        Self {
            id: id.to_owned(),
            source,
            groups: Vec::new(),
            requirement: control.map(|control| control.requirement.clone()),
            purpose: control.map(|control| control.purpose.clone()),
            kind: control.map_or_default(Control::kind),
            show: control.map_or(ShowAt::Edit, Control::show),
            checker: (control.and_then(|control| control.checker.clone()))
                .filter(|checker| !matches!(checker, Checker::Other(_))),
            result: Outcome::Written,
            code: None,
            used: None,
            max: None,
            files: Vec::new(),
            over: Vec::new(),
            behind: Vec::new(),
            omitted: Omitted::default(),
            zero: None,
        }
    }
}

/// The rules the mounted repository holds itself, each `unmeasured`, or
/// `unreadable` when Board marks it degraded, with the fields drift
/// compares.
fn own_rows(host: &dyn Host, place: &Place) -> Result<Vec<(Row, Option<Compared>)>, PluginError> {
    let Some((mount, _)) = &place.mount else {
        return Ok(Vec::new());
    };
    let beneath = literal(&format!("{}/", mount.as_str()));
    let sql = format!(
        "SELECT logical, body, status, code FROM records WHERE type IN {RULE_TYPES} \
         AND source != 'worktree' AND substr(logical, 1, length({beneath})) = {beneath} \
         ORDER BY logical"
    );
    let mut rows = Vec::new();
    for row in index::rows(host, &sql)?.rows {
        let [Value::String(logical), body, status, code] = &row[..] else {
            continue;
        };
        let control = (body.as_str())
            .and_then(|body| parse_json(body.as_bytes()).ok())
            .and_then(|record| Control::deserialize(&record).ok());
        let id = place.relative(logical);
        let mut out = Row::of(&id, Source::Repository, control.as_ref());
        out.result = Outcome::Unmeasured;
        if status != "ok" {
            out.result = Outcome::Unreadable;
            out.code = Some(code.as_str().unwrap_or("rules::rule::degraded").to_owned());
        }
        rows.push((out, control.as_ref().map(Compared::of)));
    }
    Ok(rows)
}

/// Where the repository's own rules differ from the imposed set.
fn drift(
    set: &BTreeMap<String, ImposedRule>,
    root: &LogicalPath,
    own: &[(Row, Option<Compared>)],
) -> Drift {
    let mut drift = Drift::default();
    for (row, fields) in own {
        let key = below(&row.id, root.as_str());
        match set.get(&key) {
            Some(imposed) if key != row.id => {
                if let (Some(theirs), Some(ours)) = (&imposed.fields, fields) {
                    let differ = theirs.differ(ours);
                    if !differ.is_empty() {
                        drift.differs.push(Differs {
                            id: imposed.id.clone(),
                            fields: differ,
                        });
                    }
                }
            }
            _ => drift.extra.push(row.id.clone()),
        }
    }
    drift.not_reached = (set.values())
        .filter(|rule| !rule.reached)
        .map(|rule| rule.id.clone())
        .collect();
    drift.differs.sort_by(|one, other| one.id.cmp(&other.id));
    drift
}

/// Each group in the board's own tree.
const GROUPS: &str = "SELECT logical, body FROM records WHERE type = 'rules/group/v1' \
    AND source = 'worktree' AND status = 'ok' ORDER BY logical";

/// Each group Board marks invalid in the board's own tree, with its code.
const INVALID_GROUPS: &str = "SELECT logical, COALESCE(code, '') FROM records \
    WHERE type = 'rules/group/v1' AND source = 'worktree' AND status != 'ok' \
    AND logical IS NOT NULL ORDER BY logical";

/// The refusal for the group at `logical` that Board marks invalid with
/// `code`, or whose body does not read as a group when `code` is empty.
fn group_unreadable(logical: String, code: &str) -> Refused {
    let message = if code.is_empty() {
        format!("the group {logical}'s body does not read as a rules/group/v1")
    } else {
        format!(
            "Board marks the group {logical} invalid with {code}; run `board check {logical}` for why"
        )
    };
    Refused {
        code: "rules::report::group_unreadable".to_owned(),
        rules: vec![logical],
        message,
    }
}

/// Each group's rolled-up state, with each row given the groups it is in.
/// A member pattern that matches no rule the report knows, reaching or
/// not, is a `rules::report::member_unmatched` warning, and a group that
/// Board marks invalid or that does not read is a
/// `rules::report::group_unreadable` refusal.
fn groups(
    host: &dyn Host,
    rows: &mut [Row],
    set: &BTreeMap<String, ImposedRule>,
    refused: &mut Vec<Refused>,
) -> Result<Vec<GroupRow>, PluginError> {
    let known: BTreeSet<&str> = (set.values().map(|rule| rule.id.as_str()))
        .chain(rows.iter().map(|row| row.id.as_str()))
        .collect();
    let known: Vec<String> = known.into_iter().map(str::to_owned).collect();
    let mut out = Vec::new();
    for (logical, code) in index::string_pairs::<Vec<(String, String)>>(host, INVALID_GROUPS)? {
        refused.push(group_unreadable(logical, &code));
    }
    for (logical, body) in index::string_pairs::<Vec<(String, String)>>(host, GROUPS)? {
        let Some(group) = (parse_json(body.as_bytes()).ok())
            .and_then(|record| GroupRecord::deserialize(&record).ok())
        else {
            refused.push(group_unreadable(logical, ""));
            continue;
        };
        for pattern in &group.members {
            if !known.iter().any(|id| glob(pattern, id)) {
                refused.push(Refused {
                    code: "rules::report::member_unmatched".to_owned(),
                    rules: vec![logical.clone()],
                    message: format!(
                        "the group {logical}'s member pattern {pattern} matches no rule"
                    ),
                });
            }
        }
        let mut counts = Counts::default();
        let mut results = Vec::new();
        for row in rows.iter_mut() {
            if group.members.iter().any(|pattern| glob(pattern, &row.id)) {
                row.groups.push(logical.clone());
                counts.add(row.result);
                results.push(row.result);
            }
        }
        out.push(GroupRow {
            id: logical,
            title: group.title,
            state: rolled_up(&results),
            counts,
        });
    }
    Ok(out)
}

/// The state `results`, a group's members' results, roll up to.
pub(crate) fn rolled_up(results: &[Outcome]) -> GroupState {
    if results.is_empty() {
        GroupState::NotReached
    } else if results.iter().any(|result| result.fails()) {
        GroupState::Failing
    } else if results.contains(&Outcome::Unreadable) {
        GroupState::Unreadable
    } else if results.iter().any(|result| result.passes()) {
        GroupState::Passing
    } else {
        GroupState::Written
    }
}

mod document;
mod place;
mod suites;

pub(crate) use document::*;
use place::Place;
#[cfg(test)]
use suites::run_commit;
use suites::suites;

#[cfg(test)]
mod tests;
