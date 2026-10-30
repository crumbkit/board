//! Generated boards, for the property tests that a warm index answers as a
//! cold one and that the commands agree about each entry.

use std::collections::BTreeMap;
use std::fs;

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
                // A plain record that becomes a link, here into the mount.
                link("/vendor/projects/p"),
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
                task(r#", "project": "/vendor/projects/p", "see": "/vendor/tasks/ok""#),
            ],
        ),
        (
            "links/l.json",
            vec![link("/projects/p"), link("/gone"), link("/links/l")],
        ),
        (
            "notes.md",
            vec![
                "a".to_owned(),
                // Links that reach a record, through a link and nowhere.
                "[p](projects/p.json) [l](/links/l#x) [gone](/gone)".to_owned(),
                "[a](./tasks/a.json) `[b](/tasks/b)`".to_owned(),
                // Links through symlinks, to a record and to a note.
                "[c](/tasks/c) [n](linked.md)".to_owned(),
                // Links into the mount, to a record and to a note.
                "[ok](/vendor/tasks/ok) [n](/vendor/notes.md)".to_owned(),
                // A task in its frontmatter, whose references and links are
                // both followed, one its schema refuses, and frontmatter
                // that does not parse.
                "---\n$type: lr/task/v1\ntitle: t\nsee: /tasks/a\n---\n[l](/links/l)".to_owned(),
                "---\n$type: lr/task/v1\ntitle: 5\n---\n".to_owned(),
                "---\n$type: lr/task/v1\ntitle: [\n---\n".to_owned(),
            ],
        ),
        // Symlinks: to a record, to one in another directory, to nothing,
        // to themselves and out of the repository, and a file in their place.
        (
            "tasks/c.json",
            vec![
                "LINK a.json".to_owned(),
                "LINK ../projects/p.json".to_owned(),
                "LINK gone.json".to_owned(),
                "LINK c.json".to_owned(),
                task(r#", "see": "/tasks/c""#),
            ],
        ),
        // A symlinked note, one to a directory, and one out of the board.
        (
            "linked.md",
            vec![
                "LINK notes.md".to_owned(),
                "LINK tasks".to_owned(),
                "LINK ../../outside.md".to_owned(),
                "[a](tasks/a.json)".to_owned(),
            ],
        ),
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

/// The item `text` describes: a symlink to what follows `LINK `, or a file.
fn item(text: &str) -> Item<'_> {
    text.strip_prefix("LINK ")
        .map_or(Item::File(text), Item::Symlink)
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
/// adds a valid task and an invalid one, makes its project a link to
/// another, changes the links of its note, and mounts an inner board that
/// a task refers into.
#[derive(Debug)]
pub struct Generated {
    scenario: Scenario,
    library: Scenario,
    _inner: Scenario,
    first: ObjectId,
    second: ObjectId,
    present: BTreeMap<String, bool>,
}

impl Generated {
    /// Writes the files `choice` names, and commits them too when
    /// `committed`.
    pub fn new(config: &str, choice: &[Option<usize>], committed: bool) -> Self {
        let library = Scenario::new();
        let schemas = [
            (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
            (".board/types/lr/project/v1.schema.json", PROJECT_SCHEMA),
        ];
        let project = r#"{"$type": "lr/project/v1"}"#;
        let mut files = schemas.to_vec();
        files.push(("projects/p.json", project));
        files.push(("notes.md", "[p](projects/p.json) [ok](/tasks/ok)"));
        let first = library.commit(&files);
        let ok = task(r#", "project": "/projects/p""#);
        let to_q = r#"{"$type": "board/mount/v1", "path": "/projects/q"}"#;
        let inner = Scenario::new();
        let inner_commit = inner.commit(&[("projects/p.json", project)]);
        let to_inner = format!(
            r#"{{"$type": "board/mount/v1", "repo": "{}", "follow": "main", "commit": "{inner_commit}"}}"#,
            inner.path()
        );
        let into_inner = task(r#", "see": "/inner/projects/p""#);
        let mut more = schemas.to_vec();
        more.push(("inner.json", &to_inner));
        more.push(("tasks/inner.json", &into_inner));
        more.push(("projects/p.json", to_q));
        more.push(("projects/q.json", project));
        more.push(("notes.md", "[ok](tasks/ok.json) [gone](/gone)"));
        more.push(("tasks/ok.json", &ok));
        more.push(("tasks/invalid.json", r#"{"$type": "lr/task/v1"}"#));
        let second = library.commit(&more);
        let mut generated = Self {
            scenario: Scenario::new(),
            library,
            _inner: inner,
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
            let items: Vec<(&[u8], Item<'_>)> = files
                .iter()
                .map(|(path, text)| (path.as_bytes(), item(text)))
                .collect();
            generated.scenario.commit_items(&items);
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
                    let item = item(&text);
                    // Writing a file through a symlink would write its target,
                    // and a symlink cannot be made over a file.
                    let linked = fs::symlink_metadata(self.scenario.workdir().join(path))
                        .is_ok_and(|metadata| metadata.is_symlink());
                    if was && (linked || matches!(item, Item::Symlink(_))) {
                        self.scenario.write(path, Item::Deleted);
                    }
                    self.scenario.write(path, item);
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
