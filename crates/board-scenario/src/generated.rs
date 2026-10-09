//! Generated boards, for the property tests that a warm index answers as a
//! cold one and that the commands agree about each entry.

use std::collections::BTreeMap;

use board_model::ObjectId;
use proptest::prelude::*;

use crate::{Item, Scenario};

/// The task type's schema: a required title and two references.
pub const TASK_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["title"],
  "properties": {
    "title": {"type": "string"},
    "project": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"},
    "see": {"type": "string", "format": "board-ref"}
  }
}"#;

/// The project type's schema, which admits any object.
pub const PROJECT_SCHEMA: &str = r#"{"type": "object"}"#;

/// A valid task with `fields` added after its title.
pub fn task(fields: &str) -> String {
    format!(r#"{{"$type": "lr/task/v1", "title": "t"{fields}}}"#)
}

/// The files a generated board can hold, and what each may contain.
fn variants() -> Vec<(&'static str, Vec<String>)> {
    let link = |target: &str| format!(r#"{{"$type": "board/mount/v1", "path": "{target}"}}"#);
    vec![
        (
            ".board/types/lr/task/v1.schema.json",
            vec![
                TASK_SCHEMA.to_owned(),
                // A schema change that changes the references of records
                // that did not change.
                TASK_SCHEMA.replace(
                    r#""see": {"type": "string", "format": "board-ref"}"#,
                    r#""see": {"type": "string"}"#,
                ),
                r#"{"type": 7}"#.to_owned(),
                // The bytes of a task, which `projects/p.json` can hold.
                task(""),
            ],
        ),
        (
            ".board/types/lr/project/v1.schema.json",
            vec![PROJECT_SCHEMA.to_owned()],
        ),
        (
            "projects/p.json",
            vec![
                r#"{"$type": "lr/project/v1"}"#.to_owned(),
                r#"{"$type": "lr/task/v1", "title": "t"}"#.to_owned(),
            ],
        ),
        (
            "tasks/a.json",
            vec![
                task(r#", "project": "/projects/p""#),
                task(r#", "see": "/nowhere""#),
                r#"{"$type": "lr/task/v1"}"#.to_owned(),
                r#"{"$type": "lr/task/v1", "title": 5, "see": 1}"#.to_owned(),
                "{".to_owned(),
            ],
        ),
        (
            "tasks/b.json",
            vec![
                task(r#", "see": "/tasks/a""#),
                task(r#", "project": "/links/l""#),
            ],
        ),
        (
            "links/l.json",
            vec![link("/projects/p"), link("/gone"), link("/links/l")],
        ),
        ("notes.md", vec!["a".to_owned(), "b".to_owned()]),
        // A name with no logical path. Its last content is the same bytes
        // as a task `projects/p.json` can hold, so one blob can be a record
        // at one path and a plain file at another.
        (
            "tasks/.json",
            vec!["{}".to_owned(), "[]".to_owned(), task("")],
        ),
        (
            "vendor.json",
            vec![
                "MOUNT first".to_owned(),
                "MOUNT second".to_owned(),
                "MOUNT gone".to_owned(),
            ],
        ),
    ]
}

/// For each file a generated board can hold, which content it holds, if
/// any. Most files are present, so most records have something to refer to.
pub fn generated() -> impl Strategy<Value = Vec<Option<usize>>> {
    let choices: Vec<_> = variants()
        .into_iter()
        .map(|(_, contents)| prop::option::weighted(0.8, 0..contents.len()))
        .collect();
    choices
}

/// A board with `config` as its `board.json` and the files [`generated`]
/// chose, and a library board with two commits for it to mount. The second
/// adds a valid task and an invalid one.
#[derive(Debug)]
pub struct Generated {
    scenario: Scenario,
    library: Scenario,
    first: ObjectId,
    second: ObjectId,
    present: BTreeMap<String, bool>,
}

impl Generated {
    /// Writes the files `choice` names, and commits them too when
    /// `committed`.
    pub fn new(config: &str, choice: &[Option<usize>], committed: bool) -> Self {
        let library = Scenario::new();
        let files = [
            (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
            (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
            ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
        ];
        let first = library.commit(&files);
        let ok = task(r#", "project": "/projects/p""#);
        let mut more = files.to_vec();
        more.push(("tasks/ok.json", &ok));
        more.push(("tasks/invalid.json", r#"{"$type": "lr/task/v1"}"#));
        let second = library.commit(&more);
        let mut generated = Self {
            scenario: Scenario::new(),
            library,
            first,
            second,
            present: BTreeMap::new(),
        };
        generated
            .scenario
            .write(".board/board.json", Item::File(config));
        generated.write(choice);
        if committed {
            let files: Vec<(String, String)> = variants()
                .into_iter()
                .zip(choice)
                .filter_map(|((path, contents), chosen)| {
                    Some((
                        path.to_owned(),
                        generated.materialize(&contents[(*chosen)?]),
                    ))
                })
                .collect();
            let files: Vec<(&str, &str)> = files
                .iter()
                .map(|(path, text)| (path.as_str(), text.as_str()))
                .collect();
            generated.scenario.commit(&files);
        }
        generated
    }

    pub fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Writes the files `choice` names over whatever the working tree holds.
    pub fn write(&mut self, choice: &[Option<usize>]) {
        for ((path, contents), chosen) in variants().into_iter().zip(choice) {
            let was = self.present.insert(path.to_owned(), chosen.is_some()) == Some(true);
            match chosen {
                Some(at) => {
                    let text = self.materialize(&contents[*at]);
                    self.scenario.write(path, Item::File(&text));
                }
                None if was => {
                    self.scenario.write(path, Item::Deleted);
                }
                None => {}
            }
        }
    }

    /// `content` with a mount placeholder made real.
    fn materialize(&self, content: &str) -> String {
        let mount = |repo: &str, commit: ObjectId| {
            format!(
                r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
            )
        };
        match content {
            "MOUNT first" => mount(&self.library.path(), self.first),
            "MOUNT second" => mount(&self.library.path(), self.second),
            "MOUNT gone" => mount("/no/such/repository", self.first),
            _ => content.to_owned(),
        }
    }
}
