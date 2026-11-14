//! Writes through a real mount whose HEAD another writer moved between the
//! read and the put, as the board-repos review asked
//! (/reviews/board-95-board-repos/report). The other writer is a second
//! Board putting through the same mount, so the pins, reads and the
//! refusal of a stale put are Board's own. Board refuses with
//! `board::put::head_moved` only when HEAD moves inside its own commit,
//! which a test cannot time, so that refusal is scripted after the other
//! writer's real put.

use std::cell::RefCell;

use board_model::{LogicalPath, Refusal};
use board_ops::Elsewhere;
use board_ops::{
    Absent, Board, DecideReport, GetReport, Limit, MountError, MountUpdateReport, PutError,
    PutReport, QueryError, QueryReport, ReadError, RefsError, RefsReport, Settings, Shown,
    SyncError, Synced, Variables,
};
use board_repos::{Repo, create};
use board_scenario::{Item, Scenario};

use super::*;
use crate::add::append;

/// What the other writer does just before the first put of the plugin.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Race {
    /// It writes nothing.
    Alone,
    /// It puts its text at the path; the plugin's put then goes to Board.
    Puts,
    /// It puts its text at the path, and the plugin's put is refused as
    /// Board refuses one whose HEAD moved while it committed.
    PutsAndHeadMoves,
    /// It writes nothing, and every put is refused as one whose HEAD moved.
    HeadMovesEveryTime,
}

/// A board mounting a notes clone at `/repos/notes`, whose puts, reads and
/// pins are Board's.
struct Mounted {
    _remote: Scenario,
    clone: Scenario,
    board: Scenario,
    settings: Settings,
    race: Race,
    /// The text the other writer puts at the first put.
    other: String,
    puts: RefCell<Vec<String>>,
}

impl Mounted {
    fn new(race: Race, other: &str) -> Self {
        let remote = Scenario::new();
        let started = remote.commit(&[
            (".board/board.json", r#"{"$type": "board/config/v1"}"#),
            ("daily/.gitkeep", ""),
        ]);
        let clone = Scenario::clone_of(&remote);
        clone.identified();
        let board = Scenario::board();
        board.identified();
        let mount = json!({"$type": "board/mount/v1", "repo": clone.path(), "follow": "main", "commit": started.to_string()});
        board.write("repos/notes.json", Item::File(&mount.to_string()));
        Self {
            _remote: remote,
            clone,
            board,
            settings: Settings {
                now: SystemTime::UNIX_EPOCH,
                day_start: None,
                utc_offset: 0,
                caller: None,
                environment: Variables::default(),
                cache_home: None,
            },
            race,
            other: other.to_owned(),
            puts: RefCell::default(),
        }
    }

    fn open(&self) -> Board {
        Board::open(self.board.workdir(), None).unwrap()
    }

    fn repo() -> Repo {
        Repo {
            root: "/repos/notes".to_owned(),
            clone: Some("/clone".to_owned()),
        }
    }

    /// The file at `path` in the clone's working tree.
    fn text(&self, path: &str) -> String {
        std::fs::read_to_string(self.clone.workdir().join(path)).unwrap()
    }
}

impl Host for Mounted {
    fn query(&self, sql: &str, limit: Limit) -> Result<QueryReport, QueryError> {
        assert_eq!(limit, Limit::ALL);
        self.open().query(sql, usize::MAX, None, None)
    }
    fn refs(
        &self,
        _: &[LogicalPath],
        _: Absent,
        _: &[String],
    ) -> Result<Vec<Result<RefsReport, RefsError>>, RefsError> {
        panic!("notes resolves no refs")
    }
    fn get_each(
        &self,
        paths: &[LogicalPath],
    ) -> Result<Vec<Result<GetReport, ReadError>>, ReadError> {
        Ok(self.open().get_each(paths)?.collect())
    }
    fn put(&self, lines: &[Value]) -> Result<PutReport, PutError> {
        let path = lines[0]["path"].as_str().unwrap().to_owned();
        let first = self.puts.borrow().is_empty();
        self.puts.borrow_mut().push(path.clone());
        let batch = |lines: &[Value]| {
            let text: Vec<String> = lines.iter().map(Value::to_string).collect();
            self.open().put_batch(&mut text.join("\n").as_bytes())
        };
        if self.race == Race::HeadMovesEveryTime {
            return Err(PutError::HeadMoved { writes: Vec::new() });
        }
        if first && self.race != Race::Alone {
            let theirs = json!({"path": path, "expect": "none", "text": self.other});
            batch(&[theirs]).unwrap();
            if self.race == Race::PutsAndHeadMoves {
                return Err(PutError::HeadMoved { writes: Vec::new() });
            }
        }
        batch(lines)
    }
    fn call(
        &self,
        _: &board_plugin::Target,
        _: &Value,
    ) -> Result<board_plugin::CallReport, Box<Refusal>> {
        panic!("notes calls no plugin")
    }
    fn board(&self, _: &Elsewhere) -> Result<Box<dyn Host + '_>, Box<Refusal>> {
        panic!("a write syncs nothing")
    }
    fn decide(
        &self,
        _: &str,
        _: &LogicalPath,
        _: Option<&Shown>,
    ) -> Result<DecideReport, Box<Refusal>> {
        panic!("notes asks the owner nothing")
    }
    fn sync(&self, _: bool, _: Duration) -> Result<Synced, SyncError> {
        panic!("a write syncs nothing")
    }
    fn mount_update(
        &self,
        path: &LogicalPath,
        deadline: Duration,
    ) -> Result<MountUpdateReport, MountError> {
        self.open().mount_update(path, deadline)
    }
    fn settings(&self) -> &Settings {
        &self.settings
    }
}

const SECTION: &str = "## 09:00\n\nMine.\n";

#[test]
fn an_append_whose_note_another_writer_changed_is_appended_again_after_theirs() {
    for race in [Race::Puts, Race::PutsAndHeadMoves] {
        let host = Mounted::new(race, "Theirs.\n");
        let clones = Through::new(&host);
        let written = append(
            writer(&host, &clones),
            &Mounted::repo(),
            "daily/d.md",
            "",
            SECTION,
        );
        let (why, commit) = written.unwrap();
        assert_eq!(why, None);
        assert!(commit.is_some(), "the put committed");
        assert_eq!(host.text("daily/d.md"), "Theirs.\n\n## 09:00\n\nMine.\n");
        assert_eq!(
            host.puts.borrow().len(),
            2,
            "one put was refused, then one landed"
        );
    }
}

#[test]
fn an_append_with_no_other_writer_lands_at_the_first_put() {
    let host = Mounted::new(Race::Alone, "");
    let clones = Through::new(&host);
    let (why, commit) = append(
        writer(&host, &clones),
        &Mounted::repo(),
        "daily/d.md",
        "",
        SECTION,
    )
    .unwrap();
    assert_eq!((why, commit.is_some()), (None, true));
    assert_eq!(host.text("daily/d.md"), SECTION);
    assert_eq!(host.puts.borrow().len(), 1);
}

#[test]
fn a_new_inbox_note_whose_name_another_writer_took_takes_the_next_name() {
    let host = Mounted::new(Race::Puts, "Theirs.\n");
    let clones = Through::new(&host);
    let (path, commit) = create(
        writer(&host, &clones),
        &Mounted::repo(),
        "inbox/2026-11-04-090000",
        "Mine.\n",
        10,
        "notes::note::busy",
    )
    .unwrap();
    assert_eq!(path, "inbox/2026-11-04-090000-2.md");
    assert!(commit.is_some());
    assert_eq!(host.text("inbox/2026-11-04-090000.md"), "Theirs.\n");
    assert_eq!(host.text(&path), "Mine.\n");
}

#[test]
fn a_new_inbox_note_with_no_other_writer_takes_its_own_name() {
    let host = Mounted::new(Race::Alone, "");
    let clones = Through::new(&host);
    let (path, commit) = create(
        writer(&host, &clones),
        &Mounted::repo(),
        "inbox/2026-11-04-090000",
        "Mine.\n",
        10,
        "notes::note::busy",
    )
    .unwrap();
    assert_eq!(
        (path.as_str(), commit.is_some()),
        ("inbox/2026-11-04-090000.md", true)
    );
}

/// The case the review named: after a put refused because HEAD moved, the
/// retry finds the name taken and reads it as its own write, as the Python
/// plugin did, though the other writer's text is there. Board's puts
/// through one mount take its lock, so only a writer outside Board, such as
/// Reflect, which never writes `inbox/`, can move HEAD inside a put.
#[test]
fn an_inbox_put_whose_head_moved_reads_a_name_another_writer_took_as_its_own() {
    let host = Mounted::new(Race::PutsAndHeadMoves, "Theirs.\n");
    let clones = Through::new(&host);
    let written = create(
        writer(&host, &clones),
        &Mounted::repo(),
        "inbox/2026-11-04-090000",
        "Mine.\n",
        10,
        "notes::note::busy",
    )
    .unwrap();
    assert_eq!(written, ("inbox/2026-11-04-090000.md".to_owned(), None));
    assert_eq!(host.text("inbox/2026-11-04-090000.md"), "Theirs.\n");
    assert_eq!(host.puts.borrow().len(), 2);
}

#[test]
fn an_append_to_the_boards_own_tree_whose_head_moved_is_written_once_and_one_through_a_mount_is_retried()
 {
    let host = Mounted::new(Race::HeadMovesEveryTime, "");
    let clones = Through::new(&host);
    let own = Repo {
        root: "/".to_owned(),
        clone: None,
    };
    let written = append(writer(&host, &clones), &own, "daily/d.md", "", SECTION).unwrap();
    assert_eq!(written, (None, None), "written, not committed");
    assert_eq!(*host.puts.borrow(), ["/daily/d.md"]);
    let host = Mounted::new(Race::HeadMovesEveryTime, "");
    let clones = Through::new(&host);
    let written = append(
        writer(&host, &clones),
        &Mounted::repo(),
        "daily/d.md",
        "",
        SECTION,
    );
    assert_eq!(
        written.unwrap(),
        (Some("board::put::head_moved".to_owned()), None)
    );
    assert_eq!(host.puts.borrow().len(), board_repos::ATTEMPTS);
}
