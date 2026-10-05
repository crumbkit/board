//! Generated boards, for the property tests that a warm index answers as a
//! cold one.

use std::collections::BTreeMap;

use board_model::ObjectId;
use board_scenario::{Item, Scenario};
use proptest::prelude::*;

use crate::tests::{PROJECT_SCHEMA, TASK_SCHEMA, task};

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

/// For each file in [`variants`], which content it holds, if any. Most
/// files are present, so most records have something to refer to.
pub(crate) fn generated() -> impl Strategy<Value = Vec<Option<usize>>> {
    let choices: Vec<_> = variants()
        .into_iter()
        .map(|(_, contents)| prop::option::weighted(0.8, 0..contents.len()))
        .collect();
    choices
}

/// A board with `config` as its `board.json` and the files [`generated`]
/// chose, and a library board with two commits for it to mount.
pub(crate) struct Generated {
    scenario: Scenario,
    library: Scenario,
    first: ObjectId,
    second: ObjectId,
    present: BTreeMap<String, bool>,
}

impl Generated {
    /// Writes the files `choice` names, and commits them too when
    /// `committed`.
    pub(crate) fn new(config: &str, choice: &[Option<usize>], committed: bool) -> Self {
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

    pub(crate) fn scenario(&self) -> &Scenario {
        &self.scenario
    }

    /// Writes the files `choice` names over whatever the working tree holds.
    pub(crate) fn write(&mut self, choice: &[Option<usize>]) {
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
