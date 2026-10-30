//! Generated boards, for the property tests that a warm index answers as a
//! cold one and that the commands agree about each entry.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use board_model::ObjectId;
use proptest::prelude::*;
use tempfile::TempDir;

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
                "MOUNT third".to_owned(),
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

/// Where the library repository is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    /// Where it was made, beside the inner repository.
    Home,
    /// One directory deeper, where its relative nested mount names nothing.
    Moved,
    /// Nowhere a pointer names.
    Gone,
}

/// One change to a generated board or to the repositories it mounts.
#[derive(Clone, Debug)]
pub enum Step {
    /// Writes the files [`generated`] chose over the working tree.
    Write(Vec<Option<usize>>),
    /// Puts the library in a place, and rewrites the pointers to name it
    /// unless it is gone.
    Library(Place),
    /// Deletes the inner repository, or brings it back.
    InnerGone(bool),
    /// Loses the first commit of the library and of the inner board, or
    /// brings them back.
    FirstLost(bool),
    /// Mounts the board holding a long path under a route short enough to
    /// name it, under one too long, or nowhere.
    LongRoute(Option<bool>),
}

/// A sequence of one to eight changes.
pub fn steps() -> impl Strategy<Value = Vec<Step>> {
    let place = prop_oneof![Just(Place::Home), Just(Place::Moved), Just(Place::Gone)];
    let step = prop_oneof![
        1 => generated().prop_map(Step::Write),
        1 => place.prop_map(Step::Library),
        1 => any::<bool>().prop_map(Step::InnerGone),
        1 => any::<bool>().prop_map(Step::FirstLost),
        1 => any::<Option<bool>>().prop_map(Step::LongRoute),
    ];
    prop::collection::vec(step, 1..9)
}

/// A pointer that mounts `commit` from the repository at `repo`.
fn mount(repo: &Path, commit: ObjectId) -> String {
    let repo = repo.to_str().unwrap();
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

/// A path that fits under `/l` and not under `/longer/route`: nineteen
/// segments of 204 bytes and one of 195.
fn long_path() -> String {
    let mut segments = vec!["a".repeat(204); 19];
    segments.push("b".repeat(195));
    format!("{}.json", segments.join("/"))
}

/// Where the repositories a [`Generated`] board mounts are, so a step can
/// move them and the board can put them back.
#[derive(Debug)]
struct Places {
    /// Holds the library when it is moved, and whatever is gone.
    aside: TempDir,
    library: PathBuf,
    inner: PathBuf,
    place: Place,
    inner_gone: bool,
    /// The first commits of the library and of the inner board.
    firsts: [ObjectId; 2],
    first_lost: bool,
}

#[expect(
    clippy::disallowed_methods,
    reason = "the scenario builder is the one writer of test files, and moves repositories and objects"
)]
impl Places {
    fn library_at(&self) -> PathBuf {
        match self.place {
            Place::Home => self.library.clone(),
            Place::Moved => self.aside.path().join("moved").join("library"),
            Place::Gone => self.aside.path().join("library"),
        }
    }

    fn put(&mut self, place: Place) {
        let from = self.library_at();
        self.place = place;
        let to = self.library_at();
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::rename(from, to).unwrap();
    }

    fn inner_at(&self) -> PathBuf {
        if self.inner_gone {
            self.aside.path().join("inner")
        } else {
            self.inner.clone()
        }
    }

    fn inner(&mut self, gone: bool) {
        let from = self.inner_at();
        self.inner_gone = gone;
        fs::rename(from, self.inner_at()).unwrap();
    }

    /// Moves the loose object of each first commit out of its repository,
    /// wherever that is, or back in.
    fn lose(&mut self, lost: bool) {
        if lost == self.first_lost {
            return;
        }
        let repositories = [self.library_at(), self.inner_at()];
        for (index, (repository, commit)) in repositories.iter().zip(self.firsts).enumerate() {
            let hex = commit.to_string();
            let object = repository
                .join(".git/objects")
                .join(&hex[..2])
                .join(&hex[2..]);
            let aside = self.aside.path().join(format!("lost-{index}"));
            let (from, to) = if lost {
                (&object, &aside)
            } else {
                (&aside, &object)
            };
            fs::rename(from, to).unwrap();
        }
        self.first_lost = lost;
    }
}

/// A board with `config` as its `board.json` and the files [`generated`]
/// chose, and a library board with three commits for it to mount. The
/// second adds a valid task and an invalid one, makes its project a link to
/// another, changes the links of its note, and mounts an inner board that
/// a task refers into. The third mounts the inner board's next commit by a
/// relative path. A third board holds a long path, which [`Step::LongRoute`]
/// mounts.
#[derive(Debug)]
pub struct Generated {
    scenario: Scenario,
    _library: Scenario,
    _inner: Scenario,
    long_board: Scenario,
    first: ObjectId,
    second: ObjectId,
    third: ObjectId,
    long: ObjectId,
    present: BTreeMap<String, bool>,
    choice: Vec<Option<usize>>,
    places: Places,
}

impl Drop for Generated {
    fn drop(&mut self) {
        // Puts every repository back, so each temporary directory deletes
        // what it made.
        self.places.lose(false);
        self.places.inner(false);
        self.places.put(Place::Home);
    }
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
        let next = inner.commit(&[("projects/p.json", project), ("projects/r.json", project)]);
        let name = inner.workdir().file_name().unwrap().to_str().unwrap();
        let to_next = format!(
            r#"{{"$type": "board/mount/v1", "repo": "../{name}", "follow": "main", "commit": "{next}"}}"#
        );
        more[2] = ("inner.json", &to_next);
        let third = library.commit(&more);
        let long_board = Scenario::new();
        let path = long_path();
        let long = long_board.commit(&[schemas[1], (path.as_str(), project)]);
        let places = Places {
            aside: tempfile::tempdir().unwrap(),
            library: library.workdir().to_path_buf(),
            inner: inner.workdir().to_path_buf(),
            place: Place::Home,
            inner_gone: false,
            firsts: [first, inner_commit],
            first_lost: false,
        };
        let mut generated = Self {
            scenario: Scenario::new(),
            _library: library,
            _inner: inner,
            long_board,
            first,
            second,
            third,
            long,
            present: BTreeMap::new(),
            choice: Vec::new(),
            places,
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

    /// Makes one change.
    pub fn apply(&mut self, step: &Step) {
        match step {
            Step::Write(choice) => self.write(choice),
            Step::Library(place) => {
                self.places.put(*place);
                if *place != Place::Gone {
                    let choice = self.choice.clone();
                    self.write(&choice);
                }
            }
            Step::InnerGone(gone) => self.places.inner(*gone),
            Step::FirstLost(lost) => self.places.lose(*lost),
            Step::LongRoute(too_long) => {
                let pointer = mount(self.long_board.workdir(), self.long);
                for (path, at) in [("l.json", Some(false)), ("longer/route.json", Some(true))] {
                    if *too_long == at {
                        self.scenario.write(path, Item::File(&pointer));
                    } else if self.scenario.workdir().join(path).exists() {
                        self.scenario.write(path, Item::Deleted);
                    }
                }
            }
        }
    }

    /// Writes the files `choice` names over whatever the working tree holds.
    pub fn write(&mut self, choice: &[Option<usize>]) {
        self.choice = choice.to_vec();
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
        let library = self.places.library_at();
        match content {
            "MOUNT first" => mount(&library, self.first),
            "MOUNT second" => mount(&library, self.second),
            "MOUNT third" => mount(&library, self.third),
            "MOUNT gone" => mount(Path::new("/no/such/repository"), self.first),
            _ => content.to_owned(),
        }
    }
}
