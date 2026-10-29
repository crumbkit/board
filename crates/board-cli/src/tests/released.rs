//! The released interfaces, listed in released.json at the repository
//! root, change only by a new version (STANDARDS.md 3 and 4): a listed
//! schema file and a listed vector file keep their bytes, a listed report
//! keeps every field, and a listed code stays. `SNAPSHOTS=overwrite` lists everything present, which
//! is the step a release takes, and `RELEASED=all` refuses a release that
//! ships something the file does not list.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::*;

const FILE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../released.json");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Where the repository keeps schema files, relative to its root.
const SCHEMAS: &[&str] = &[".board/types", "plugins"];

/// Where the repository keeps the vectors other repositories test against.
const VECTORS: &str = "vectors";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Released {
    /// Each schema file by its path from the repository root, with the git
    /// blob id of its bytes.
    types: BTreeMap<String, String>,
    /// Each `--json` report by its `schema`, with the paths of the fields
    /// its `data` holds; `[]` stands for every element of an array.
    reports: BTreeMap<String, BTreeSet<String>>,
    codes: BTreeSet<String>,
    /// Each vector file but a README, by its path from the repository root,
    /// with the git blob id of its bytes.
    vectors: BTreeMap<String, String>,
}

/// What a listed entry would have to change in place to be as it is now.
fn violations(released: &Released, present: &Released) -> Vec<String> {
    let mut found = Vec::new();
    for (path, blob) in &released.types {
        match present.types.get(path) {
            Some(now) if now == blob => {}
            Some(_) => found.push(format!("{path} changed; a changed type is a new version")),
            None => found.push(format!("{path} is gone; an old version stays readable")),
        }
    }
    for (report, fields) in &released.reports {
        for field in fields {
            let held = present.reports.get(report);
            if !held.is_some_and(|held| held.contains(field)) {
                found.push(format!("{report} no longer holds {field}"));
            }
        }
    }
    for (path, blob) in &released.vectors {
        match present.vectors.get(path) {
            Some(now) if now == blob => {}
            Some(_) => found.push(format!("{path} changed; a changed vector is a new version")),
            None => found.push(format!("{path} is gone; a released vector stays")),
        }
    }
    for code in &released.codes {
        if !present.codes.contains(code) {
            found.push(format!("{code} is gone; a code is never renamed"));
        }
    }
    found
}

/// Fields whose contents are not Board's: a record, a plugin's schemas and
/// metadata, and what jsonschema says about a record.
const FOREIGN: &[&str] = &[
    "record",
    "inputSchema",
    "outputSchema",
    "_meta",
    "metadata",
    "failures",
];

/// Every path at which `value` holds something, under `prefix`.
fn paths(prefix: &str, value: &Value, into: &mut BTreeSet<String>) {
    match value {
        Value::Object(fields) => {
            for (key, value) in fields {
                let path = format!("{prefix}.{key}");
                if !FOREIGN.contains(&key.as_str()) {
                    paths(&path, value, into);
                }
                into.insert(path);
            }
        }
        Value::Array(elements) => {
            for element in elements {
                paths(&format!("{prefix}[]"), element, into);
            }
        }
        _ => {}
    }
}

/// Every file under `directory` that `wanted` admits, by its path from the
/// repository root, with its blob id.
fn blobs(
    hasher: &Scenario,
    directory: &Path,
    wanted: fn(&Path) -> bool,
    into: &mut BTreeMap<String, String>,
) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            blobs(hasher, &path, wanted, into);
        } else if wanted(&path) {
            let relative = path.strip_prefix(ROOT).unwrap().to_string_lossy();
            let blob = hasher.blob_of(&std::fs::read(&path).unwrap());
            into.insert(relative.replace('\\', "/"), blob.to_string());
        }
    }
}

fn is_schema(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".schema.json")
}

fn is_vector(path: &Path) -> bool {
    path.file_name().is_some_and(|name| name != "README.md")
}

/// Every report once, from a board holding a record, a mount one commit
/// behind its library, and the tasks plugin shipping a schema that
/// differs from the board's copy. The put lands a degraded record, so the
/// check and the refs report a code. A second clone's sync replays a
/// commit, so its report holds `replayed`. `query` and `call` are left
/// out: their `data` is the statement's rows and the plugin's output.
fn reports() -> BTreeMap<String, BTreeSet<String>> {
    let library = Scenario::new();
    let old = library.commit(&[("a.md", "one")]);
    library.commit(&[("a.md", "two")]);
    let (board, data) = plugins::holding(r#"{"type": "array"}"#);
    board
        .write(
            ".board/types/lr/task/v1.schema.json",
            Item::File(TASK_SCHEMA),
        )
        .write(
            ".board/types/lr/project/v1.schema.json",
            Item::File(r#"{"type": "object"}"#),
        )
        .write(
            "projects/p.json",
            Item::File(r#"{"$type": "lr/project/v1"}"#),
        )
        .write(
            "tasks/a.json",
            Item::File(r#"{"$type": "lr/task/v1", "title": "a", "project": "/projects/p"}"#),
        )
        .write(
            "vendor/lib.json",
            Item::File(&mount_of(&library.path(), old)),
        );
    let plugins = data.workdir().join("plugins");
    let record = br#"{"$type": "lr/task/v1", "title": "b", "project": "/missing"}"#;
    let runs: &[(&[&str], &[u8])] = &[
        (&["init"], b""),
        (&["put", "/tasks/b"], record),
        (&["check"], b""),
        (&["get", "/projects/p", "/tasks/a"], b""),
        (&["ls", "/"], b""),
        (&["refs", "/tasks/a", "/tasks/b"], b""),
        (
            &["decide", "--op", "review.comment", "--path", "/tasks/a"],
            b"",
        ),
        (&["mount", "update", "/vendor/lib"], b""),
        (&["plugin", "list"], b""),
        (&["plugin", "find"], b""),
        (&["plugin", "enable", "tasks"], b""),
    ];
    let mut outputs: Vec<(Vec<&str>, Output)> = runs
        .iter()
        .map(|(arguments, stdin)| {
            let arguments = [&["--json"], *arguments].concat();
            let output = board_full(board.workdir(), None, Some(&plugins), &arguments, stdin);
            (arguments, output)
        })
        .collect();
    let (_remote, a, b) = sync::clones();
    let project = br#"{"$type": "lr/project/v1"}"#;
    for (clone, at) in [(&a, "/projects/a"), (&b, "/projects/b")] {
        board_with(clone.workdir(), None, &["put", at], project);
    }
    board_in(a.workdir(), &["sync"]);
    let arguments = vec!["--json", "sync"];
    outputs.push((arguments.clone(), board_in(b.workdir(), &arguments)));
    let mut reports = BTreeMap::new();
    for (arguments, output) in outputs {
        let report = json(&output);
        let schema = report["schema"]
            .as_str()
            .unwrap_or_else(|| panic!("{arguments:?} printed no report: {}", text(&output.stderr)));
        assert!(schema.ends_with("-report/v2"), "{arguments:?}: {schema}");
        let mut fields = BTreeSet::new();
        paths("data", &report["data"], &mut fields);
        reports.insert(schema.to_owned(), fields);
    }
    reports
}

fn present() -> Released {
    let hasher = Scenario::new();
    let mut types = BTreeMap::new();
    for directory in SCHEMAS {
        blobs(
            &hasher,
            &Path::new(ROOT).join(directory),
            is_schema,
            &mut types,
        );
    }
    let mut vectors = BTreeMap::new();
    blobs(
        &hasher,
        &Path::new(ROOT).join(VECTORS),
        is_vector,
        &mut vectors,
    );
    Released {
        types,
        reports: reports(),
        codes: Code::ALL
            .iter()
            .map(|code| code.as_str().to_owned())
            .collect(),
        vectors,
    }
}

/// The mode the environment asks for: `SNAPSHOTS=overwrite` writes the
/// file, as crumbkit-test writes its goldens, and `RELEASED=all` is the
/// release's check that the file lists everything the checkout ships.
#[expect(
    clippy::disallowed_methods,
    reason = "released.json is a golden file, written and selected as crumbkit-test's are"
)]
fn mode() -> (bool, bool) {
    (
        std::env::var("SNAPSHOTS").as_deref() == Ok("overwrite"),
        std::env::var("RELEASED").as_deref() == Ok("all"),
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "released.json is a golden file, written as crumbkit-test's are"
)]
fn write(released: &Released) {
    let text = serde_json::to_string_pretty(released).unwrap();
    std::fs::write(FILE, format!("{text}\n")).unwrap();
}

/// What `SNAPSHOTS=overwrite` may write: everything present, unless an
/// entry `released` lists changed in place, which it refuses as the check
/// does, so an overwrite never freezes an edit.
fn overwritten(released: &Released, present: Released) -> Result<Released, Vec<String>> {
    let found = violations(released, &present);
    if found.is_empty() {
        Ok(present)
    } else {
        Err(found)
    }
}

#[test]
fn every_released_type_report_and_code_is_as_it_was_released() {
    let present = present();
    let (overwrite, all) = mode();
    let released: Released = serde_json::from_slice(&std::fs::read(FILE).unwrap()).unwrap();
    let present = overwritten(&released, present).unwrap_or_else(|found| {
        panic!(
            "released.json lists an interface that changed in place:\n{}",
            found.join("\n")
        )
    });
    if overwrite {
        write(&present);
        return;
    }
    if all {
        assert_eq!(
            released, present,
            "released.json does not list what this release ships; run this test with SNAPSHOTS=overwrite"
        );
    }
}

#[test]
fn a_changed_schema_a_dropped_field_and_a_gone_code_are_refused_and_the_unchanged_are_admitted() {
    let hasher = Scenario::new();
    let (one, two) = (hasher.blob("{}").to_string(), hasher.blob("[]").to_string());
    let listed = |blob: &str, field: &str, code: &str| Released {
        types: [(
            "plugins/x/types/x/y/v1.schema.json".to_owned(),
            blob.to_owned(),
        )]
        .into(),
        reports: [("board/x-report/v2".to_owned(), [field.to_owned()].into())].into(),
        codes: [code.to_owned()].into(),
        vectors: BTreeMap::new(),
    };
    let now = listed(&one, "data.kept", "board::x::kept");
    assert_eq!(violations(&now, &now), Vec::<String>::new());
    let changed = listed(&two, "data.gone", "board::x::gone");
    assert_eq!(
        violations(&changed, &now),
        [
            "plugins/x/types/x/y/v1.schema.json changed; a changed type is a new version",
            "board/x-report/v2 no longer holds data.gone",
            "board::x::gone is gone; a code is never renamed",
        ]
    );
    let gone = Released {
        types: [("plugins/x/types/x/z/v1.schema.json".to_owned(), one)].into(),
        ..Released::default()
    };
    assert_eq!(
        violations(&gone, &now),
        ["plugins/x/types/x/z/v1.schema.json is gone; an old version stays readable"]
    );
}

#[test]
fn a_changed_or_gone_vector_is_refused_and_an_unchanged_one_is_admitted() {
    let hasher = Scenario::new();
    let (one, two) = (hasher.blob("a").to_string(), hasher.blob("b").to_string());
    let listed = |path: &str, blob: &str| Released {
        vectors: [(path.to_owned(), blob.to_owned())].into(),
        ..Released::default()
    };
    let now = listed("vectors/signing/v1/approval.sig", &one);
    assert_eq!(violations(&now, &now), Vec::<String>::new());
    assert_eq!(
        violations(&listed("vectors/signing/v1/approval.sig", &two), &now),
        ["vectors/signing/v1/approval.sig changed; a changed vector is a new version"]
    );
    assert_eq!(
        violations(&listed("vectors/signing/v1/write.sig", &one), &now),
        ["vectors/signing/v1/write.sig is gone; a released vector stays"]
    );
}

#[test]
fn an_overwrite_that_would_change_a_released_vector_is_refused_and_one_that_adds_a_vector_is_admitted()
 {
    let hasher = Scenario::new();
    let (one, two) = (hasher.blob("a").to_string(), hasher.blob("b").to_string());
    let listed = |entries: &[(&str, &str)]| Released {
        vectors: entries
            .iter()
            .map(|(path, blob)| ((*path).to_owned(), (*blob).to_owned()))
            .collect(),
        ..Released::default()
    };
    let released = listed(&[("vectors/signing/v1/approval.sig", &one)]);
    let added = listed(&[
        ("vectors/signing/v1/approval.sig", &one),
        ("vectors/signing/v2/approval.sig", &two),
    ]);
    assert_eq!(overwritten(&released, added.clone()), Ok(added));
    let edited = listed(&[("vectors/signing/v1/approval.sig", &two)]);
    assert_eq!(
        overwritten(&released, edited),
        Err(vec![
            "vectors/signing/v1/approval.sig changed; a changed vector is a new version".to_owned()
        ])
    );
}
