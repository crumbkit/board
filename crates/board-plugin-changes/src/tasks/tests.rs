//! How a task path splits, and which tasks a plan closes at merge.

use std::cell::RefCell;
use std::collections::BTreeMap;

use board_model::{Code, Fields, LogicalPath, Refusal};
use board_ops::{Host, Limit, QueryError, QueryReport, ReadError, RefsError, Settings};
use board_plugin::{CallReport, Target};
use serde_json::{Value, json};

use super::{close_at_merge, closes_at_merge, split_task};
use crate::gates::sections;

#[test]
fn a_task_path_splits_into_its_repo_and_its_path_in_the_repo() {
    assert_eq!(
        split_task("/repos/notes/tasks/a.md").unwrap(),
        ("/repos/notes", "/tasks/a.md")
    );
    assert_eq!(split_task("/tasks/a.md").unwrap(), ("/", "/tasks/a.md"));
}

#[test]
fn a_path_under_no_tasks_directory_is_refused() {
    let refused = split_task("/repos/notes/a.md").unwrap_err();
    assert_eq!(refused.code, "changes::task::not_a_task_path");
}

/// The paths the plan `text` lists under Closes at merge.
fn named(text: &str) -> Vec<String> {
    closes_at_merge(&sections(text))
}

#[test]
fn a_plan_names_each_board_path_under_closes_at_merge_bare_or_in_backticks_once() {
    let plan = "# P\n\n## Closes at merge\n\n- `/repos/n/tasks/a.md`, whose work this does.\n- \
                /tasks/b.md.\n- (/tasks/c.md) and `/repos/n/tasks/a.md` again\n\n## Data\n\n\
                /tasks/d.md\n";
    assert_eq!(
        named(plan),
        ["/repos/n/tasks/a.md", "/tasks/b.md", "/tasks/c.md"]
    );
}

#[test]
fn a_path_matches_only_as_a_whole_word_and_a_plan_without_the_section_names_none() {
    let plan = "# P\n\n## Closes at merge\n\n/repos/n/tasks/a.md\n";
    let found = named(plan);
    assert!(!found.contains(&"/tasks/a.md".to_owned()), "{found:?}");
    assert_eq!(found, ["/repos/n/tasks/a.md"]);
    let none: [String; 0] = [];
    assert_eq!(named("# P\n\n## Outcome\n\n/tasks/a.md\n"), none);
    assert_eq!(named("# P\n\n## Closes at merge\n\nNone.\n"), none);
}

/// A host that holds a task at each path of `statuses`, and whose
/// `tasks.done` marks one done, except at the paths in `refuses`, which it
/// refuses.
struct Tasks {
    settings: Settings,
    statuses: RefCell<BTreeMap<String, String>>,
    refuses: RefCell<Vec<String>>,
    /// The paths `tasks.done` was called for, in order.
    calls: RefCell<Vec<String>>,
}

impl Tasks {
    fn new(statuses: &[(&str, &str)], refuses: &[&str]) -> Self {
        Self {
            settings: Settings {
                now: std::time::SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: board_ops::Variables::default(),
                cache_home: None,
            },
            statuses: RefCell::new(
                (statuses.iter())
                    .map(|(path, status)| ((*path).to_owned(), (*status).to_owned()))
                    .collect(),
            ),
            refuses: RefCell::new(refuses.iter().map(|path| (*path).to_owned()).collect()),
            calls: RefCell::new(Vec::new()),
        }
    }

    fn status(&self, path: &str) -> String {
        self.statuses.borrow()[path].clone()
    }
}

impl Host for Tasks {
    fn query(&self, sql: &str, _limit: Limit) -> Result<QueryReport, QueryError> {
        let statuses = self.statuses.borrow();
        let found = (statuses.iter())
            .find(|(path, _)| sql.contains(&format!("logical = '{path}'")))
            .map(|(_, status)| vec![json!(status)]);
        Ok(QueryReport {
            columns: vec!["status".into()],
            rows: found.into_iter().collect(),
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, RefsError>>, RefsError> {
        unreachable!("closing reads no references")
    }

    fn get_each(
        &self,
        _paths: &[LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, ReadError>>, ReadError> {
        unreachable!("closing reads no files")
    }

    fn put(&self, _lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        unreachable!("closing writes only through tasks.done")
    }

    fn call(&self, target: &Target, input: &Value) -> Result<CallReport, Box<Refusal>> {
        let Target::Named { plugin, capability } = target else {
            unreachable!("closing names the plugin it calls")
        };
        assert_eq!(format!("{plugin}.{capability}"), "tasks.done");
        let path = input["path"].as_str().unwrap().to_owned();
        self.calls.borrow_mut().push(path.clone());
        if self.refuses.borrow().contains(&path) {
            let refused = Refusal::new(Code::RecordInvalid, "the push failed", &Fields::default());
            return Err(Box::new(refused));
        }
        self.statuses.borrow_mut().insert(path, "done".to_owned());
        Ok(CallReport {
            plugin: plugin.clone(),
            capability: capability.clone(),
            output: json!({}),
            warnings: Vec::new(),
        })
    }

    fn board(&self, _elsewhere: &board_ops::Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        unreachable!("closing opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<Refusal>> {
        unreachable!("closing asks the owner nothing")
    }

    fn sync(
        &self,
        _: bool,
        _: std::time::Duration,
    ) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("closing syncs nothing")
    }

    fn mount_update(
        &self,
        _: &LogicalPath,
        _: std::time::Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("closing updates no mount")
    }

    fn settings(&self) -> &Settings {
        &self.settings
    }
}

const RERUN: &str = "board changes advance --path /changes/t";

#[test]
fn closing_at_merge_closes_each_open_task_in_order_and_skips_one_already_closed() {
    let host = Tasks::new(
        &[
            ("/tasks/a.md", "open"),
            ("/tasks/b.md", "done"),
            ("/tasks/c.md", "open"),
        ],
        &[],
    );
    let named = ["/tasks/a.md", "/tasks/b.md", "/tasks/c.md"].map(str::to_owned);
    let closed = close_at_merge(&host, "/changes/t", "land", &named, RERUN).unwrap();
    assert_eq!(closed, ["/tasks/a.md", "/tasks/c.md"]);
    assert_eq!(*host.calls.borrow(), ["/tasks/a.md", "/tasks/c.md"]);
}

#[test]
fn a_refused_close_names_the_task_and_a_rerun_closes_only_the_rest() {
    let host = Tasks::new(
        &[("/tasks/a.md", "open"), ("/tasks/b.md", "open")],
        &["/tasks/b.md"],
    );
    let named = ["/tasks/a.md", "/tasks/b.md"].map(str::to_owned);
    let refused = close_at_merge(&host, "/changes/t", "land", &named, RERUN).unwrap_err();
    assert_eq!(refused.code, "changes::land::tasks_not_closed");
    assert_eq!(refused.fields["task"], "/tasks/b.md");
    assert!(
        refused.message.contains("stays in land"),
        "{}",
        refused.message
    );
    assert!(
        refused.message.contains("the push failed"),
        "{}",
        refused.message
    );
    assert!(refused.message.contains(RERUN), "{}", refused.message);
    assert_eq!(
        (host.status("/tasks/a.md"), host.status("/tasks/b.md")),
        ("done".to_owned(), "open".to_owned())
    );
    host.refuses.borrow_mut().clear();
    let closed = close_at_merge(&host, "/changes/t", "land", &named, RERUN).unwrap();
    assert_eq!(closed, ["/tasks/b.md"]);
}
