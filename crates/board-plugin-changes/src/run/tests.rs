//! The agent's report is read from its last line of stdout, and a run
//! names at most one stop.

use super::*;

#[test]
fn the_cost_and_duration_are_read_from_the_last_line_only_when_it_is_a_json_object() {
    let printed = "working\n{\"total_cost_usd\": 0.42, \"duration_ms\": 1200}\n\n";
    assert_eq!(reported(printed), (Some(0.42), Some(1200)));
    let earlier = "{\"total_cost_usd\": 0.42}\ndone\n";
    assert_eq!(reported(earlier), (None, None));
    assert_eq!(reported(""), (None, None));
}

fn asked(through: Option<&str>, until: Option<&str>) -> Input {
    serde_json::from_value(json!({"path": "/changes/x", "through": through, "until": until}))
        .expect("the input is valid")
}

#[test]
fn both_stops_are_refused_and_either_one_alone_is_admitted() {
    let refused = asked_stop(&asked(Some("plan"), Some("review"))).expect_err("two stops");
    assert_eq!(refused.code, "changes::run::two_stops");
    assert_eq!(
        asked_stop(&asked(Some("plan"), None)).expect("one stop"),
        Some(Stop::Through("plan".to_owned()))
    );
    assert_eq!(
        asked_stop(&asked(None, Some("review"))).expect("one stop"),
        Some(Stop::Until("review".to_owned()))
    );
    assert_eq!(asked_stop(&asked(None, None)).expect("no stop"), None);
}

#[test]
fn the_stopping_text_names_the_budget_only_when_one_is_given() {
    let without = stopping("/changes/x", "/repos/x", None);
    assert!(without.contains("changes stop --path /changes/x --reason reached"));
    assert!(!without.contains("US dollars"));
    assert!(stopping("/changes/x", "/repos/x", Some(2.5)).contains("at most 2.5 US dollars"));
}

#[test]
fn a_run_time_is_the_same_instant_as_its_stamp_to_the_second() {
    let at = SystemTime::UNIX_EPOCH + Duration::from_micros(1_793_967_334_421_737);
    assert_eq!(stamp(at), "20261106T121534421737Z");
    assert_eq!(time_of(at), "2026-11-06T12:15:34Z");
}

/// A host holding records by logical path, each with its blob, that
/// answers the blob and body queries `copy_back` makes and applies each put
/// line's expect as Board does under its put lock: a line whose expect the
/// held blob does not match refuses the whole batch as stale.
struct Holding {
    settings: board_ops::Settings,
    held: std::cell::RefCell<std::collections::BTreeMap<String, (String, Value)>>,
    /// A record another writer stores just before the next put.
    first: std::cell::RefCell<Option<(String, Value)>>,
    landed: std::cell::RefCell<Vec<Vec<Value>>>,
}

/// The blob another writer's record gets.
const THEIRS: &str = "1111111111111111111111111111111111111111";

impl Holding {
    fn new(records: &[(&str, Value)]) -> Self {
        Self {
            settings: board_ops::Settings {
                now: SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: board_ops::Variables::default(),
                cache_home: None,
            },
            held: std::cell::RefCell::new(
                (records.iter())
                    .map(|(path, record)| ((*path).to_owned(), (THEIRS.to_owned(), record.clone())))
                    .collect(),
            ),
            first: std::cell::RefCell::default(),
            landed: std::cell::RefCell::default(),
        }
    }

    fn blob(&self, path: &str) -> Option<String> {
        self.held.borrow().get(path).map(|(blob, _)| blob.clone())
    }
}

impl Host for Holding {
    fn query(
        &self,
        sql: &str,
        _limit: board_ops::Limit,
    ) -> Result<board_ops::QueryReport, board_ops::QueryError> {
        let path = (sql.rsplit_once("logical = '"))
            .and_then(|(_, rest)| rest.strip_suffix('\''))
            .unwrap_or_default()
            .replace("''", "'");
        let held = self.held.borrow();
        let rows = match held.get(&path) {
            Some((blob, _)) if sql.starts_with("SELECT blob FROM entries") => {
                vec![vec![json!(blob)]]
            }
            Some((_, record)) if sql.starts_with("SELECT body FROM records") => {
                vec![vec![json!(record.to_string())]]
            }
            _ => Vec::new(),
        };
        Ok(board_ops::QueryReport {
            columns: vec!["value".into()],
            rows,
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }

    fn refs(
        &self,
        _paths: &[board_model::LogicalPath],
        _absent: board_ops::Absent,
        _fields: &[String],
    ) -> Result<Vec<Result<board_ops::RefsReport, board_ops::RefsError>>, board_ops::RefsError>
    {
        unreachable!("copy_back resolves no paths")
    }

    fn get_each(
        &self,
        _paths: &[board_model::LogicalPath],
    ) -> Result<Vec<Result<board_ops::GetReport, board_ops::ReadError>>, board_ops::ReadError> {
        unreachable!("copy_back reads no files")
    }

    fn put(&self, lines: &[Value]) -> Result<board_ops::PutReport, board_ops::PutError> {
        if let Some((path, record)) = self.first.borrow_mut().take() {
            (self.held.borrow_mut()).insert(path, (THEIRS.to_owned(), record));
        }
        for line in lines {
            let path = line["path"].as_str().unwrap();
            let held = self.blob(path);
            let matches = match line["expect"].as_str() {
                None => true,
                Some("none") => held.is_none(),
                Some(blob) => held.as_deref() == Some(blob),
            };
            if !matches {
                return Err(board_ops::PutError::Stale {
                    path: path.parse().unwrap(),
                    found: held.as_deref().and_then(board_model::ObjectId::from_hex),
                });
            }
        }
        for line in lines {
            let path = line["path"].as_str().unwrap().to_owned();
            let blob = "2222222222222222222222222222222222222222".to_owned();
            (self.held.borrow_mut()).insert(path, (blob, line["record"].clone()));
        }
        self.landed.borrow_mut().push(lines.to_vec());
        Ok(board_ops::PutReport {
            writes: Vec::new(),
            removed: Vec::new(),
            absent: Vec::new(),
            commit: None,
            mended: Vec::new(),
        })
    }

    fn call(
        &self,
        _target: &board_plugin::Target,
        _input: &Value,
    ) -> Result<board_plugin::CallReport, Box<board_model::Refusal>> {
        unreachable!("copy_back calls no plugin")
    }

    fn board(
        &self,
        _elsewhere: &board_ops::Elsewhere,
    ) -> Result<Box<dyn Host + '_>, Box<board_model::Refusal>> {
        unreachable!("copy_back opens no other board")
    }

    fn decide(
        &self,
        _op: &str,
        _path: &board_model::LogicalPath,
        _shown: Option<&board_ops::Shown>,
    ) -> Result<board_ops::DecideReport, Box<board_model::Refusal>> {
        unreachable!("copy_back asks the owner nothing")
    }

    fn sync(&self, _: bool, _: Duration) -> Result<board_ops::Synced, board_ops::SyncError> {
        unreachable!("copy_back syncs nothing")
    }

    fn mount_update(
        &self,
        _: &board_model::LogicalPath,
        _: Duration,
    ) -> Result<board_ops::MountUpdateReport, board_ops::MountError> {
        unreachable!("copy_back updates no mount")
    }

    fn settings(&self) -> &board_ops::Settings {
        &self.settings
    }
}

/// The flow version the sandbox's run names.
const VERSION: &str = "/flows/development/v-0123456789ab";

/// A sandbox holding a run that names `VERSION`, its stop and the version.
fn sandboxed() -> Holding {
    Holding::new(&[
        (
            "/changes/t/runs/1",
            json!({"$type": "changes/run/v1", "flow": VERSION}),
        ),
        ("/changes/t/stops/1", json!({"$type": "changes/stop/v1"})),
        (
            VERSION,
            json!({"$type": "changes/flow/v1", "name": "development"}),
        ),
    ])
}

/// Copies the sandbox's run back into `host` for `/changes/x`.
fn copied_back(host: &Holding) -> Result<(String, String), PluginError> {
    sandbox::copy_back(
        host,
        &sandboxed(),
        "/changes/x",
        "/changes/t",
        ("/changes/t/runs/1", "/changes/t/stops/1"),
    )
}

#[test]
fn copying_back_writes_the_missing_flow_version_expecting_nothing_there() {
    let host = Holding::new(&[]);
    // The attach after the put asks for the change, which this host lacks.
    let _ = copied_back(&host);
    let landed = host.landed.borrow();
    let version = (landed[0].iter())
        .find(|line| line["path"] == VERSION)
        .expect("the version is copied");
    assert_eq!(version["expect"], "none", "{version}");
}

#[test]
fn a_flow_version_another_writer_stores_first_makes_the_copy_back_stale_and_writes_nothing() {
    let host = Holding::new(&[]);
    *host.first.borrow_mut() = Some((VERSION.to_owned(), json!({"$type": "changes/flow/v1"})));
    let refused = copied_back(&host).unwrap_err();
    assert_eq!(
        refused.fields["code"], "board::put::stale",
        "{}",
        refused.message
    );
    assert!(host.landed.borrow().is_empty(), "no line is written");
    let held: Vec<String> = host.held.borrow().keys().cloned().collect();
    assert_eq!(held, [VERSION], "only the other writer's version is there");
}
