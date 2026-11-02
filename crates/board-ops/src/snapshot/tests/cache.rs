//! What a run may reuse from the index: a pointer list only when the facts
//! cover every entry, and a decision only when every read succeeded.

use board_index::{BlobFact, Index};
use board_model::{Kind, Location, Object, ObjectId, Role, Source};
use board_scenario::Item;
use board_tree::{Cache, Entry, Status};

use crate::index::Indexed;
use crate::snapshot::{Decide, Facts, Indexing};
use crate::tests::{board, open, task};

#[test]
fn facts_are_complete_when_they_hold_every_entry_with_a_blob_and_a_role_and_not_when_one_is_missing()
 {
    let blob = ObjectId::from_hex("0123456789abcdef0123456789abcdef01234567").unwrap();
    let entry = |path: &str, blob| Entry {
        path: path.to_owned(),
        logical: None,
        blob,
        status: Status::Ok,
        stat: None,
        size: None,
        link: None,
    };
    let indexed = Indexed {
        blob,
        role: Role::Json,
        fact: BlobFact {
            kind: Kind::Record,
            record_type: None,
            code: None,
            reason: None,
            links: Vec::new(),
        },
        validation: None,
    };
    let facts: Facts = [("held.json".to_owned(), indexed)].into_iter().collect();
    let complete = |entries: &[Entry]| <Indexing<'_> as Cache>::complete(&facts, entries);
    let covered = [
        entry("held.json", Some(blob)),
        entry("notes.txt", Some(blob)),
        entry("sealed.json", None),
    ];
    assert!(complete(&covered));
    assert!(!complete(&[
        entry("held.json", Some(blob)),
        entry("missing.json", Some(blob))
    ]));
}

#[test]
fn a_failed_read_marks_the_decision_unkeepable_and_a_later_read_that_succeeds_does_not_clear_it() {
    let scenario = board();
    let text = task("");
    scenario.write("tasks/a.json", Item::File(&text));
    let board = open(&scenario);
    let index = Index::open(None, board.repository().path()).unwrap();
    let writer = index.write().unwrap();
    let facts = std::collections::HashMap::new();
    let decide = Decide {
        repositories: board.repositories(),
        writer: &writer,
        listed: &[],
        facts: &facts,
        mounted: None,
        resolve: std::cell::RefCell::new(Box::new(|_: &board_model::LogicalPath| {
            panic!("reading a type resolves nothing")
        })),
        reader: std::cell::RefCell::new(board.repository().reader()),
        failed: std::cell::Cell::new(false),
        remotes: std::cell::OnceCell::new(),
    };
    let read = |path: &str, blob| {
        let location = Location {
            board: board_model::Board {
                repository: board.repository().path().to_path_buf(),
                source: Source::WorkingTree,
            },
            path: path.to_owned(),
            object: Object::Blob(blob),
            pointers: Vec::new(),
        };
        decide.record_type(&location).unwrap()
    };
    let blob = scenario.blob(&text);
    assert_eq!(read("tasks/a.json", blob), "lr/task/v1".parse().ok());
    assert!(!decide.failed.get(), "a read that succeeds fails nothing");
    assert_eq!(read("tasks/gone.json", blob), None);
    assert!(decide.failed.get());
    read("tasks/a.json", blob);
    assert!(decide.failed.get(), "a later read keeps the failure");
}
