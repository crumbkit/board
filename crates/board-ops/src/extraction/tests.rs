use std::path::PathBuf;

use board_scenario::{Item, Scenario};
use serde_json::{Value, json};

use crate::tests::{TASK_SCHEMA, board, open, task};
use crate::{Plugins, QueryReport};

/// Keeps each request as `requests/<n>.json`, answers an extraction by
/// naming every blob it was sent, and a call with a count.
const EXTRACTING: &str = r#"#!/bin/sh
mkdir -p requests
n=$(ls requests | wc -l | tr -d ' ')
cat > "requests/$n.json"
if grep -q '"board/call/v1"' "requests/$n.json"; then
  printf '%s' '{"$type": "board/call-result/v1", "output": {}}'
  exit 0
fi
blobs=$(sed -n 's/^ *"blob": "\([0-9a-f]*\)",*$/"\1"/p' "requests/$n.json" | paste -s -d, -)
printf '{"$type": "board/extract-result/v1", "blobs": [%s]}' "$blobs"
"#;

/// A board of tasks enabling `tasks`, which extracts `lr/task/v1` with
/// `script`, and a cache.
struct Fixture {
    board: Scenario,
    data: Scenario,
    cache: Scenario,
}

impl Fixture {
    fn new(script: &str) -> Self {
        let board = board();
        board
            .write(
                ".board/board.json",
                Item::File(r#"{"$type": "board/config/v1", "plugins": ["tasks"]}"#),
            )
            .write("tasks/a.json", Item::File(&task("")))
            .write("tasks/b.json", Item::File(&task(r#", "see": "/nowhere""#)))
            .write("tasks/bad.json", Item::File(r#"{"$type": "lr/task/v1"}"#))
            .write(
                "projects/q.json",
                Item::File(r#"{"$type": "lr/project/v1"}"#),
            );
        let data = Scenario::new();
        data.write(
            "plugins/tasks/plugin.json",
            Item::File(
                &json!({
                    "$type": "board/plugin-manifest/v1",
                    "name": "tasks",
                    "version": "0.1.0",
                    "description": "",
                    "run": ["./run"],
                    "extract": ["lr/task/v1"],
                    "capabilities": [{
                        "name": "due", "description": "",
                        "input_schema": true, "output_schema": true
                    }]
                })
                .to_string(),
            ),
        )
        .write("plugins/tasks/run", Item::Executable(script));
        Self {
            board,
            data,
            cache: Scenario::new(),
        }
    }

    fn plugins(&self) -> PathBuf {
        self.data.workdir().join("plugins")
    }

    fn query(&self, sql: &str) -> QueryReport {
        let directory = self.plugins();
        let plugins = Plugins {
            directory: Some(&directory),
            environment: board_plugin::Environment {
                path: Some("/usr/bin:/bin".into()),
                ..board_plugin::Environment::default()
            },
            limits: board_plugin::Limits::default(),
        };
        open(&self.board)
            .query(sql, 500, Some(self.cache.workdir()), &plugins)
            .unwrap()
    }

    /// Each extraction request the plugin received, in order.
    fn extractions(&self) -> Vec<Value> {
        let directory = self.plugins().join("tasks/requests");
        let mut requests = Vec::new();
        for n in 0.. {
            let Ok(bytes) = std::fs::read(directory.join(format!("{n}.json"))) else {
                break;
            };
            let request: Value = serde_json::from_slice(&bytes).unwrap();
            if request["$type"] == "board/extract/v1" {
                requests.push(request);
            }
        }
        requests
    }

    /// The paths each extraction request held.
    fn sent(&self) -> Vec<Vec<String>> {
        self.extractions()
            .iter()
            .map(|request| {
                request["records"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|record| record["path"].as_str().unwrap().to_owned())
                    .collect()
            })
            .collect()
    }
}

#[test]
fn query_sends_each_valid_record_of_the_plugins_types_once() {
    let fixture = Fixture::new(EXTRACTING);
    let report = fixture.query("SELECT count(*) FROM p_tasks.sqlite_schema");
    assert_eq!(report.rows, [[json!(0)]]);
    assert!(report.plugins.is_empty(), "{:?}", report.plugins);
    // A dangling reference depends on the tree, not the record's bytes.
    assert_eq!(fixture.sent(), [["/tasks/a", "/tasks/b"]]);
    let request = &fixture.extractions()[0];
    let record = &request["records"][0];
    assert_eq!(record["type"], "lr/task/v1");
    assert_eq!(
        record["record"],
        json!({"$type": "lr/task/v1", "title": "t"})
    );
    assert_eq!(record["blob"].as_str().unwrap().len(), 40);
    let database = PathBuf::from(request["board"]["database"].as_str().unwrap());
    assert!(
        database.starts_with(fixture.cache.workdir()),
        "{database:?}"
    );
    assert!(database.ends_with("plugins/tasks@0.1.0.sqlite"));
}

#[test]
fn a_record_already_handled_is_not_sent_again() {
    let fixture = Fixture::new(EXTRACTING);
    fixture.query("SELECT 1");
    fixture.query("SELECT 1");
    assert_eq!(fixture.sent().len(), 1);
    fixture.board.write(
        "tasks/a.json",
        Item::Fresh(&task(r#", "project": "/projects/q""#)),
    );
    let report = fixture.query("SELECT blob FROM extracted ORDER BY blob");
    assert_eq!(fixture.sent()[1], ["/tasks/a"]);
    assert_eq!(report.rows.len(), 2, "{:?}", report.rows);
}

#[test]
fn a_missing_database_makes_board_send_everything_again() {
    let fixture = Fixture::new(EXTRACTING);
    let report = fixture.query("SELECT 1");
    let database = &fixture.extractions()[0]["board"]["database"];
    let database = PathBuf::from(database.as_str().unwrap());
    let relative = database.strip_prefix(fixture.cache.workdir()).unwrap();
    fixture
        .cache
        .write(relative.to_str().unwrap(), Item::Deleted);
    fixture.query("SELECT 1");
    assert_eq!(
        fixture.sent(),
        [["/tasks/a", "/tasks/b"], ["/tasks/a", "/tasks/b"]]
    );
    assert!(report.fallbacks.is_empty());
}

#[test]
fn a_failed_extraction_is_reported_and_the_query_still_answers() {
    let failing = "#!/bin/sh\ncat > /dev/null\necho broken >&2\nexit 3\n";
    let fixture = Fixture::new(failing);
    let report = fixture.query("SELECT count(*) FROM p_tasks.sqlite_schema");
    assert_eq!(report.rows, [[json!(0)]]);
    assert_eq!(report.plugins.len(), 1);
    assert_eq!(report.plugins[0].plugin.as_str(), "tasks");
    assert_eq!(report.plugins[0].code, "board::plugin::failed");
    let report = fixture.query("SELECT count(*) FROM extracted");
    assert_eq!(report.rows, [[json!(0)]]);
    let fixture = Fixture::new(EXTRACTING);
    assert!(fixture.query("SELECT 1").plugins.is_empty());
}

#[test]
fn naming_a_blob_that_was_not_sent_is_bad_output() {
    let unsent = format!(
        "#!/bin/sh\ncat > /dev/null\nprintf '%s' '{{\"$type\": \"board/extract-result/v1\", \"blobs\": [\"{}\"]}}'\n",
        "1".repeat(40)
    );
    let report = Fixture::new(&unsent).query("SELECT 1");
    assert_eq!(report.plugins[0].code, "board::plugin::bad_output");
    let none = "#!/bin/sh\ncat > /dev/null\nprintf '%s' '{\"$type\": \"board/extract-result/v1\", \"blobs\": []}'\n";
    assert!(Fixture::new(none).query("SELECT 1").plugins.is_empty());
}

#[test]
fn an_enabled_plugin_that_is_not_installed_is_reported_by_query() {
    let fixture = Fixture::new(EXTRACTING);
    fixture.data.write("plugins/tasks", Item::Deleted);
    let report = fixture.query("SELECT 1");
    assert_eq!(report.plugins[0].code, "board::plugin::not_installed");
    assert_eq!(report.rows, [[json!(1)]]);
}

#[test]
fn records_in_a_mounted_board_are_extracted_by_their_display_path() {
    let fixture = Fixture::new(EXTRACTING);
    let mounted = Scenario::new();
    let commit = mounted.commit(&[
        (".board/board.json", r#"{"$type": "board/config/v1"}"#),
        (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
        ("tasks/m.json", r#"{"$type": "lr/task/v1", "title": "m"}"#),
    ]);
    fixture.board.write(
        "shared.json",
        Item::File(
            &json!({
                "$type": "board/mount/v1",
                "repo": mounted.path(),
                "follow": "main",
                "commit": commit.to_string(),
            })
            .to_string(),
        ),
    );
    fixture.query("SELECT 1");
    assert_eq!(
        fixture.sent(),
        [["/shared/tasks/m", "/tasks/a", "/tasks/b"]]
    );
}

#[test]
fn a_database_that_cannot_be_kept_beside_the_index_is_a_noted_temporary_file() {
    let fixture = Fixture::new(EXTRACTING);
    let root = open(&fixture.board).repository().path().to_path_buf();
    let index = board_index::location(fixture.cache.workdir(), &root);
    let plugins = index.parent().unwrap().join("plugins");
    let relative = plugins.strip_prefix(fixture.cache.workdir()).unwrap();
    fixture
        .cache
        .write(relative.to_str().unwrap(), Item::File("not a directory"));
    let report = fixture.query("SELECT count(*) FROM p_tasks.sqlite_schema");
    assert_eq!(
        report
            .fallbacks
            .iter()
            .map(board_plugin::PluginName::as_str)
            .collect::<Vec<_>>(),
        ["tasks"]
    );
    assert_eq!(report.rows, [[json!(0)]]);
    let fixture = Fixture::new(EXTRACTING);
    assert!(fixture.query("SELECT 1").fallbacks.is_empty());
}

#[test]
fn call_extracts_for_the_called_plugin_first_and_refuses_with_its_failure() {
    let fixture = Fixture::new(EXTRACTING);
    let directory = fixture.plugins();
    let plugins = Plugins {
        directory: Some(&directory),
        environment: board_plugin::Environment {
            path: Some("/usr/bin:/bin".into()),
            ..board_plugin::Environment::default()
        },
        limits: board_plugin::Limits::default(),
    };
    let target: board_plugin::Target = "tasks.due".parse().unwrap();
    let called = open(&fixture.board)
        .call(&target, b"{}", &plugins, Some(fixture.cache.workdir()))
        .unwrap();
    assert_eq!(called.report.output, json!({}));
    assert_eq!(fixture.sent(), [["/tasks/a", "/tasks/b"]]);

    let failing = Fixture::new("#!/bin/sh\ncat > /dev/null\nexit 3\n");
    let directory = failing.plugins();
    let plugins = Plugins {
        directory: Some(&directory),
        ..plugins
    };
    let error = open(&failing.board)
        .call(&target, b"{}", &plugins, Some(failing.cache.workdir()))
        .unwrap_err();
    assert_eq!(error.code_text(), "board::plugin::failed");
}
