//! The manifest parses as Board reads it, the committed schema is what the
//! record struct generates, only a record the owner signed is trusted, a
//! run refuses while another holds the lock, `add` writes only a signed
//! record, and the agent's plist and launchctl calls. The rounds over real
//! boards, git and gitleaks are in board-cli's
//! `src/tests/built_in/autosync.rs`.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::Path;
use std::time::{Duration, SystemTime};

use board_model::{LogicalPath, Refusal, canonical_json};
use board_ops::{
    Absent, DecideReport, Elsewhere, GetReport, Limit, MountError, MountUpdateReport,
    OwnerWriteError, PutError, PutReport, QueryError, QueryReport, ReadError, RefsError,
    RefsReport, Run, Settings, Shown, SyncError, Synced, Variables,
};
use board_plugin::child::{Exited, ProcessError};
use serde_json::json;

use super::*;

/// The committed schema file holds, byte for byte, the canonical JSON of
/// the schema the struct generates. `SNAPSHOTS=overwrite` rewrites it.
#[expect(
    clippy::disallowed_methods,
    reason = "the schema file is a golden, selected and written as crumbkit-test's are"
)]
#[test]
fn the_committed_schema_is_what_its_struct_generates() {
    for (record_type, schema) in Autosync.schemas() {
        let file = (Path::new(env!("CARGO_MANIFEST_DIR")).join("types"))
            .join(format!("{}.schema.json", record_type.as_str()));
        let generated = canonical_json(&schema);
        if std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite") {
            std::fs::write(&file, generated.as_bytes()).unwrap();
        }
        let committed = std::fs::read(&file).unwrap();
        assert!(
            committed == generated.as_bytes(),
            "{} differs from what its struct generates; run this test with SNAPSHOTS=overwrite:\n{}",
            file.display(),
            String::from_utf8_lossy(generated.as_bytes())
        );
    }
}

/// The `description` of SKILL.md's frontmatter, a single-quoted YAML
/// scalar on one line.
fn skill_description() -> String {
    let skill = include_str!("../../../plugins/autosync/SKILL.md");
    let line = (skill.lines())
        .find_map(|line| line.strip_prefix("description: "))
        .unwrap();
    let quoted = line.strip_prefix('\'').unwrap().strip_suffix('\'').unwrap();
    quoted.replace("''", "'")
}

#[test]
fn the_manifest_parses_as_a_built_in_one_and_describes_the_plugin_with_the_skills_description() {
    let manifest = Autosync.manifest();
    assert_eq!(manifest.name.as_str(), "autosync");
    let names: Vec<&str> = (manifest.capabilities.iter())
        .map(|capability| capability.name.as_str())
        .collect();
    assert_eq!(names, ["add", "run", "status", "install", "uninstall"]);
    assert_eq!(super::manifest()["description"], skill_description());
}

/// A host over nothing but a home directory: its queries answer `rows`,
/// its puts are recorded, its processes are recorded, `launchctl
/// bootstrap` exits with `bootstrap` and every other `launchctl` with 0,
/// and it signs and verifies as a stand-in signer when it has one.
/// The stand-in's signature names the signer, the path and the record, so
/// the tests check what the plugin asks to sign and verify; real signing
/// is tested in board-ops' effects module and through the binary.
struct Mac {
    home: tempfile::TempDir,
    settings: Settings,
    signer: Option<&'static str>,
    rows: Vec<Vec<Value>>,
    puts: RefCell<Vec<Value>>,
    runs: RefCell<Vec<String>>,
    bootstrap: i32,
}

impl Mac {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let settings = Settings {
            now: SystemTime::UNIX_EPOCH,
            day_start: None,
            utc_offset: 0,
            caller: None,
            environment: Variables(vec![
                ("HOME".to_owned(), home.path().as_os_str().to_owned()),
                ("PATH".to_owned(), OsString::from("/nowhere/bin")),
            ]),
            cache_home: None,
        };
        Self {
            home,
            settings,
            signer: Some(OWNER),
            rows: Vec::new(),
            puts: RefCell::default(),
            runs: RefCell::default(),
            bootstrap: 0,
        }
    }

    fn call(&self, capability: &str, input: Value) -> Result<Value, PluginError> {
        Autosync.call(self, &capability.parse().unwrap(), input)
    }

    /// The stand-in signature by `signer` of the owner's put of `record`
    /// at `path`.
    fn signed(signer: &str, path: &str, record: &Value) -> String {
        let record = canonical_json(record);
        format!("{signer} {path} {}", record.as_str())
    }
}

/// The signer the stand-in host trusts.
const OWNER: &str = "owner";

impl Host for Mac {
    fn query(&self, sql: &str, _: Limit) -> Result<QueryReport, QueryError> {
        let tasks = sql.contains("tasks/task/v2");
        Ok(QueryReport {
            columns: vec!["logical".to_owned(), "body".to_owned()],
            rows: if tasks { Vec::new() } else { self.rows.clone() },
            truncated: false,
            over_budget: false,
            memory: None,
        })
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
        _: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("autosync resolves no refs")
    }
    fn get_each(&self, _: &[LogicalPath]) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        panic!("this host holds no task")
    }
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        self.puts.borrow_mut().extend(lines.iter().cloned());
        Ok(PutReport {
            writes: Vec::new(),
            removed: Vec::new(),
            absent: Vec::new(),
            commit: None,
        })
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        let message = "this host enables no tasks plugin";
        Err(Box::new(Refusal::new(
            board_model::Code::PluginNotEnabled,
            message,
            &board_model::Fields::default(),
        )))
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("a board on this host is never reached")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("autosync asks for no approval")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<Synced, SyncError> {
        panic!("autosync syncs only guarded")
    }
    fn mount_update(&self, _: &LogicalPath, _: Duration) -> Result<MountUpdateReport, MountError> {
        panic!("autosync updates no mount")
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
    fn run_hiding(&self, run: &Run<'_>, _: &[&str]) -> Result<Exited, ProcessError> {
        let line = std::iter::once(run.program)
            .chain(run.arguments.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        self.runs.borrow_mut().push(line);
        if run.program == "launchctl" || run.program == "id" {
            let stdout = if run.program == "id" {
                b"501\n".to_vec()
            } else {
                Vec::new()
            };
            let bootstrap = run
                .arguments
                .first()
                .is_some_and(|word| word == "bootstrap");
            return Ok(Exited {
                code: Some(if bootstrap { self.bootstrap } else { 0 }),
                stdout,
                stderr: Vec::new(),
            });
        }
        Err(ProcessError::NotStarted {
            source: std::io::Error::from(std::io::ErrorKind::NotFound),
        })
    }
    fn sign_owner_write(&self, path: &str, record: &Value) -> Result<String, OwnerWriteError> {
        let signer = self.signer.ok_or(OwnerWriteError::NoSigner)?;
        Ok(Self::signed(signer, path, record))
    }
    fn verify_owner_write(
        &self,
        path: &str,
        record: &Value,
        signature: &str,
    ) -> Result<(), OwnerWriteError> {
        let signer = self.signer.ok_or(OwnerWriteError::NoneNamed)?;
        if signature == Self::signed(signer, path, record) {
            Ok(())
        } else {
            Err(OwnerWriteError::NotOffered)
        }
    }
}

#[test]
fn a_run_while_another_holds_the_lock_is_refused_at_once_and_one_after_it_runs() {
    let mac = Mac::new();
    let lock = state(mac.home.path()).join("run.lock");
    let held = board_index::lock_state(&lock).unwrap();
    let refused = mac.call("run", json!({})).unwrap_err();
    assert_eq!(refused.code, "autosync::run::busy");
    drop(held);
    let ran = mac.call("run", json!({})).unwrap();
    assert_eq!(ran["boards"], json!([]));
}

/// A record for a board whose directory is missing, so a record whose
/// signature verifies goes on to be refused as missing.
fn record() -> Value {
    json!({"$type": "autosync/board/v1", "path": "/nowhere/at/all", "origins": ["/nowhere/remote"]})
}

fn outcomes(mac: &Mac) -> Vec<(String, String)> {
    let refused = mac.call("run", json!({})).unwrap_err();
    assert_eq!(refused.code, "autosync::run::failed");
    (refused.fields["boards"].as_array().unwrap().iter())
        .map(|entry| {
            let name = entry["name"].as_str().unwrap().to_owned();
            (name, entry["code"].as_str().unwrap_or("-").to_owned())
        })
        .collect()
}

#[test]
fn only_a_record_the_owner_signed_for_its_own_path_is_trusted() {
    let mut mac = Mac::new();
    let owner = OWNER;
    let stranger = "stranger";
    let at = |name: &str| format!("/autosync/boards/{name}");
    let with = |mut record: Value, signature: String| {
        record["signature"] = json!(signature);
        record
    };
    let signed = with(record(), Mac::signed(owner, &at("signed"), &record()));
    let mut edited = with(record(), Mac::signed(owner, &at("edited"), &record()));
    edited["origins"] = json!(["git@evil.example:o/r.git"]);
    let copied = with(record(), Mac::signed(owner, &at("signed"), &record()));
    let theirs = with(record(), Mac::signed(stranger, &at("theirs"), &record()));
    mac.rows = [
        ("signed", signed),
        ("unsigned", record()),
        ("edited", edited),
        ("copied", copied),
        ("theirs", theirs),
    ]
    .into_iter()
    .map(|(name, body)| vec![json!(at(name)), body])
    .collect();
    let mut found = outcomes(&mac);
    found.sort();
    let unsigned = "autosync::board::unsigned".to_owned();
    assert_eq!(
        found,
        [
            ("copied".to_owned(), unsigned.clone()),
            ("edited".to_owned(), unsigned.clone()),
            ("signed".to_owned(), "autosync::board::missing".to_owned()),
            ("theirs".to_owned(), unsigned.clone()),
            ("unsigned".to_owned(), unsigned),
        ]
    );
    let git: Vec<String> = (mac.runs.borrow().iter())
        .filter(|run| run.starts_with("git "))
        .cloned()
        .collect();
    assert_eq!(git, Vec::<String>::new(), "no board was checked or fetched");
}

#[test]
fn add_without_a_signer_writes_nothing_and_with_one_writes_the_signed_record() {
    let input = json!({"name": "team", "path": "~/team", "origin": "git@example.com:t/b.git"});
    let mut mac = Mac::new();
    mac.signer = None;
    let refused = mac.call("add", input.clone()).unwrap_err();
    assert_eq!(refused.code, "autosync::sign::no_signer");
    assert_eq!(*mac.puts.borrow(), Vec::<Value>::new());
    let mac = Mac::new();
    let added = mac.call("add", input).unwrap();
    assert_eq!(added["path"], "/autosync/boards/team");
    let put = mac.puts.borrow()[0].clone();
    let mut record = put["record"].clone();
    let signature = record.as_object_mut().unwrap().remove("signature").unwrap();
    mac.verify_owner_write(
        "/autosync/boards/team",
        &record,
        signature.as_str().unwrap(),
    )
    .unwrap();
}

#[test]
fn install_dry_run_prints_the_plist_with_its_interval_and_path_and_loads_nothing() {
    let mac = Mac::new();
    let printed = mac
        .call("install", json!({"interval": 120, "dry-run": true}))
        .unwrap();
    let text = printed["text"].as_str().unwrap();
    assert!(text.contains("<integer>120</integer>"), "{text}");
    assert!(text.contains("<string>/nowhere/bin</string>"), "{text}");
    assert!(text.contains("<string>autosync</string>"), "{text}");
    assert_eq!(printed["loaded"], false);
    // Nothing rotates launchd's output files, so the agent keeps none.
    assert_eq!(
        text.matches("<string>/dev/null</string>").count(),
        2,
        "{text}"
    );
    assert_eq!(*mac.runs.borrow(), Vec::<String>::new());
    assert!(!Path::new(printed["plist"].as_str().unwrap()).exists());
    let refused = mac.call("install", json!({"interval": 30})).unwrap_err();
    assert_eq!(refused.code, "autosync::agent::interval");
}

#[test]
fn install_writes_and_bootstraps_the_agent_and_uninstall_boots_it_out_and_deletes_it() {
    let mac = Mac::new();
    let installed = mac.call("install", json!({})).unwrap();
    let plist = Path::new(installed["plist"].as_str().unwrap()).to_path_buf();
    assert!(plist.starts_with(mac.home.path()));
    assert!(
        std::fs::read_to_string(&plist)
            .unwrap()
            .contains("<integer>300</integer>")
    );
    let runs = mac.runs.borrow().clone();
    assert_eq!(runs.len(), 3, "{runs:?}");
    assert_eq!(runs[0], "id -u");
    assert_eq!(runs[1], "launchctl bootout gui/501/board.autosync");
    assert!(
        runs[2].starts_with("launchctl bootstrap gui/501 "),
        "{runs:?}"
    );
    assert!(runs[2].ends_with("board.autosync.plist"), "{runs:?}");
    mac.call("uninstall", json!({})).unwrap();
    assert!(!plist.exists());
    assert_eq!(
        mac.runs.borrow()[4],
        "launchctl bootout gui/501/board.autosync"
    );
}

#[test]
fn a_failed_bootstrap_says_no_agent_is_loaded_and_a_working_one_loads_it() {
    let mut mac = Mac::new();
    mac.bootstrap = 5;
    let refused = mac.call("install", json!({})).unwrap_err();
    assert_eq!(refused.code, "autosync::agent::failed");
    assert!(
        refused.message.contains("no agent is loaded now"),
        "{}",
        refused.message
    );
    mac.bootstrap = 0;
    let installed = mac.call("install", json!({})).unwrap();
    assert_eq!(installed["loaded"], true);
}
