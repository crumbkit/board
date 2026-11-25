//! How the changes plugin reads the rules plugin's answer, and a host that
//! serves one `rules.for-paths` answer and counts the calls made to it.

use std::cell::RefCell;
use std::collections::BTreeMap;

use board_model::{LogicalPath, PluginCode, PluginName, Refusal};
use board_ops::{
    Absent, DecideReport, Elsewhere, GetReport, Host, Limit, MountError, MountUpdateReport,
    PutError, PutReport, QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings,
    Shown, SyncError, Synced, Variables,
};
use board_plugin::{CallReport, Target};
use board_tree::ResolveError;
use serde_json::json;

use super::*;

/// The plan the change in [`Ruled`]'s board names.
pub(crate) const PLAN: &str = "/changes/c/plans/p";
/// The paths that plan names.
pub(crate) const PATHS: [&str; 3] = [
    "/repos/board/src/a.rs",
    "/repos/board/src/b.rs",
    "/repos/board/docs/*.md",
];

/// A board whose index holds the plan at [`PLAN`] and the obligations
/// `owed`, whose tree holds nothing, and whose rules plugin answers
/// `rules.for-paths` with `answer` or refuses it with that code, counting
/// each call by its target.
pub(crate) struct Ruled {
    settings: Settings,
    answer: Result<Value, &'static str>,
    owed: Vec<&'static str>,
    pub(crate) calls: RefCell<BTreeMap<String, usize>>,
}

impl Ruled {
    pub(crate) fn new(answer: Result<Value, &'static str>, owed: &[&'static str]) -> Self {
        Self {
            settings: Settings {
                now: std::time::SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables::default(),
                cache_home: None,
            },
            answer,
            owed: owed.to_vec(),
            calls: RefCell::new(BTreeMap::new()),
        }
    }

    /// How many calls the host answered to `target`.
    pub(crate) fn calls_to(&self, target: &str) -> usize {
        self.calls.borrow().get(target).copied().unwrap_or(0)
    }
}

fn report(rows: Vec<Vec<Value>>) -> QueryReport {
    QueryReport {
        columns: Vec::new(),
        rows,
        truncated: false,
        over_budget: false,
        memory: None,
    }
}

fn absent(path: &LogicalPath) -> ResolveError {
    ResolveError::NotFound {
        path: path.clone(),
        ancestor: LogicalPath::root(),
    }
}

impl Host for Ruled {
    fn query(&self, sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        if sql.contains(&literal(PLAN)) && sql.contains("'changes/plan/v1'") {
            let plan = json!({
                "$type": "changes/plan/v1",
                "text": format!("{PLAN}.md"),
                "uses": [],
                "paths": PATHS,
            });
            return Ok(report(vec![vec![json!("blob"), json!(plan.to_string())]]));
        }
        if sql.contains("= 'obligation'") {
            let rows = (self.owed.iter())
                .filter(|each| sql.contains(&literal(each)))
                .map(|each| vec![json!(each)])
                .collect();
            return Ok(report(rows));
        }
        Ok(report(Vec::new()))
    }

    fn refs(
        &self,
        paths: &[LogicalPath],
        _absent: Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        Ok((paths.iter())
            .map(|path| {
                Err(RefsError::Read {
                    source: ReadError::Resolve {
                        source: absent(path),
                    },
                })
            })
            .collect())
    }

    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok((paths.iter())
            .map(|path| {
                Err(ReadError::Resolve {
                    source: absent(path),
                })
            })
            .collect())
    }

    fn put(&self, _lines: &[Value]) -> Result<PutReport, PutError> {
        unreachable!("a gate reading writes nothing")
    }

    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>> {
        let Target::Named { plugin, capability } = target else {
            unreachable!("the changes plugin names each plugin it calls")
        };
        let named = format!("{}.{}", plugin.as_str(), capability.as_str());
        *self.calls.borrow_mut().entry(named.clone()).or_default() += 1;
        assert_eq!(named, "rules.for-paths", "a gate reading calls only rules");
        assert_eq!(input, &json!({"paths": PATHS}));
        match &self.answer {
            Ok(output) => Ok(CallReport {
                plugin: plugin.clone(),
                capability: capability.clone(),
                output: output.clone(),
                warnings: Vec::new(),
            }),
            Err(code) => {
                let rules: PluginName = "rules".parse().unwrap();
                let code = PluginCode::parse(&rules, (*code).to_owned()).unwrap();
                Err(Box::new(Refusal::plugin(
                    code,
                    "the rules index is broken".to_owned(),
                    serde_json::Map::new(),
                )))
            }
        }
    }

    fn board(&self, _elsewhere: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("a gate reading opens no other board")
    }

    fn decide(
        &self,
        _capability: &str,
        _path: &LogicalPath,
        _shown: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        unreachable!("a gate reading asks the owner nothing")
    }

    fn sync(&self, _push: bool, _deadline: std::time::Duration) -> Result<Synced, SyncError> {
        unreachable!("a gate reading syncs nothing")
    }

    fn mount_update(
        &self,
        _path: &LogicalPath,
        _deadline: std::time::Duration,
    ) -> Result<MountUpdateReport, MountError> {
        unreachable!("a gate reading updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

#[test]
fn the_rules_plugin_output_reads_in_the_new_names() {
    let output = json!({"rules": [
        {"rule": "/rules/docs", "summary": "Say what exists.", "matched": ["/a"], "close": "Delete it."},
        {"rule": "/rules/rust", "summary": "Errors are typed."},
    ]});
    let found = found_in(&output).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|each| (each.rule.as_str(), each.close.as_deref()))
            .collect::<Vec<_>>(),
        [("/rules/docs", Some("Delete it.")), ("/rules/rust", None)]
    );
    assert_eq!(listed(&output).unwrap().len(), 2);
}

#[test]
fn output_in_the_former_names_is_refused_and_never_read_as_no_rules() {
    let former =
        json!({"controls": [{"control": "/controls/docs", "summary": "Say what exists."}]});
    assert_eq!(
        listed(&former).unwrap_err().code,
        "changes::rules::unreadable"
    );
    assert_eq!(
        found_in(&former).unwrap_err().code,
        "changes::rules::unreadable"
    );
    let half = json!({"rules": [{"control": "/controls/docs", "summary": "Say what exists."}]});
    assert_eq!(
        found_in(&half).unwrap_err().code,
        "changes::rules::unreadable"
    );
}

#[test]
fn the_brief_refuses_a_half_renamed_rule_and_lists_a_whole_one() {
    let whole = json!({"rules": [{"rule": "/rules/docs", "summary": "Say what exists."}]});
    assert_eq!(listed(&whole).unwrap().len(), 1);
    for half in [
        json!({"rules": [{"control": "/controls/docs", "summary": "Say what exists."}]}),
        json!({"rules": [{"rule": "/rules/docs"}]}),
    ] {
        assert_eq!(
            listed(&half).unwrap_err().code,
            "changes::rules::unreadable"
        );
    }
}

#[test]
fn one_answer_gives_the_obligations_by_their_stored_path_and_every_rule_in_its_rules_form() {
    let answer = json!({"rules": [
        {"rule": "/controls/owed", "summary": "Owe it.", "close": "Close it."},
        {"rule": "/rules/style", "summary": "Keep the style."},
    ]});
    let host = Ruled::new(Ok(answer), &["/controls/owed"]);
    let paths = PATHS.map(str::to_owned);
    let found = on_plan(&host, &paths).unwrap();
    let owed: Vec<String> = (obligations_in(&host, &found).unwrap().into_iter())
        .map(|each| each.rule)
        .collect();
    assert_eq!(owed, ["/controls/owed"]);
    assert_eq!(names(&found), ["/rules/owed", "/rules/style"]);
    assert_eq!(host.calls_to("rules.for-paths"), 1);
}
