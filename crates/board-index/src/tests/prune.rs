//! Pruning: what a prune keeps and deletes, and when a writer prunes.

use board_model::{Change, canonical_json};
use board_tree::{Entry, Status, TreeRow, TreeRows};
use rusqlite::OptionalExtension as _;

use super::*;

const TREE_MODE: u32 = 0o40_000;
const FILE_MODE: u32 = 0o100_644;

fn row(name: &str, mode: u32, object: ObjectId) -> TreeRow {
    TreeRow {
        name: name.into(),
        mode,
        object,
        size: None,
        segment: Some(name.into()),
        code: None,
    }
}

fn valid(target: &str) -> ValidationFact {
    ValidationFact::Valid(vec![RefFact {
        pointer: "/see".into(),
        target: target.parse().unwrap(),
        accepts: None,
    }])
}

/// Stores facts about a source whose root tree is `blob(10)` with a subtree
/// `blob(11)`, about a working-tree file of blob 4, and, when `unreachable`,
/// about a tree and blobs that nothing reaches: tree 12 with blob 3, and
/// blob 5. Returns how many rows the unreachable facts add.
fn stored(writer: &Writer, unreachable: bool) -> usize {
    let repository = Path::new(ROOT);
    writer
        .set_mounts(&[(Source::WorkingTree, Some(blob(10)), repository)], &[])
        .unwrap();
    let trees = [
        (
            10,
            vec![
                row("sub", TREE_MODE, blob(11)),
                row("a.json", FILE_MODE, blob(1)),
            ],
        ),
        (11, vec![row("b.json", FILE_MODE, blob(2))]),
    ];
    for (id, rows) in trees {
        writer.put_tree(&TreeRows { id: blob(id), rows }).unwrap();
    }
    let file = Entry {
        path: "w.json".into(),
        logical: None,
        blob: Some(blob(4)),
        status: Status::Ok,
        stat: None,
        size: None,
        link: None,
    };
    let worktree = writer.worktree().unwrap();
    writer.sync_worktree(worktree, &[file]).unwrap();
    for reached in [1, 2, 4] {
        writer
            .put_blob(blob(reached), Role::Json, &record())
            .unwrap();
        writer
            .put_body(blob(reached), &canonical_json(&serde_json::json!({})))
            .unwrap();
    }
    writer
        .put_blob(blob(4), Role::Markdown, &markdown())
        .unwrap();
    writer
        .put_validation(blob(1), blob(2), &valid("/a"))
        .unwrap();
    writer
        .put_validation(blob(4), blob(2), &valid("/b"))
        .unwrap();
    if !unreachable {
        return 0;
    }
    writer
        .put_tree(&TreeRows {
            id: blob(12),
            rows: vec![row("c.json", FILE_MODE, blob(3))],
        })
        .unwrap();
    writer
        .put_blob(blob(5), Role::Markdown, &markdown())
        .unwrap();
    for gone in [3, 5] {
        writer.put_blob(blob(gone), Role::Json, &record()).unwrap();
        writer
            .put_body(blob(gone), &canonical_json(&serde_json::json!({})))
            .unwrap();
    }
    // A reached record against a schema nothing reaches, and the reverse.
    writer
        .put_validation(blob(1), blob(5), &valid("/c"))
        .unwrap();
    writer
        .put_validation(blob(3), blob(2), &valid("/d"))
        .unwrap();
    // One tree row, three blobs with two links and two bodies, two
    // validations and their refs.
    1 + 3 + 2 + 2 + 2 + 2
}

/// The rows of every table a prune may delete from.
fn counts(index: &Index) -> Vec<(String, i64)> {
    ["trees", "blobs", "bodies", "validations", "refs", "links"]
        .into_iter()
        .map(|table| {
            let sql = format!("SELECT count(*) FROM main.{table}");
            let count = index
                .connection
                .query_row(&sql, [], |row| row.get(0))
                .unwrap();
            (table.to_owned(), count)
        })
        .collect()
}

/// `index` holding the facts `stored` gives, with blob 3 unreachable when
/// `unreachable`. Returns it and the rows nothing reaches.
fn filled_with(index: Index, unreachable: bool) -> (Index, usize) {
    let writer = index.write().unwrap();
    let rows = stored(&writer, unreachable);
    (writer.decide(Vec::new()).unwrap().index, rows)
}

/// Prunes `index`, and returns it and the rows the prune deleted.
fn prune(index: Index) -> (Index, usize) {
    let writer = index.write().unwrap();
    let pruned = writer.prune().unwrap();
    (writer.decide(Vec::new()).unwrap().index, pruned)
}

fn memory() -> Index {
    Index::open(None, Path::new(ROOT)).unwrap()
}

#[test]
fn a_prune_deletes_the_facts_no_source_or_working_tree_file_reaches() {
    let (index, unreachable) = filled_with(memory(), true);
    let (reached, _) = filled_with(memory(), false);
    let (index, pruned) = prune(index);
    assert_eq!(pruned, unreachable);
    assert_eq!(counts(&index), counts(&reached));
}

#[test]
fn a_prune_keeps_every_fact_a_source_or_working_tree_file_reaches() {
    let (index, _) = filled_with(memory(), false);
    let before = counts(&index);
    let (index, pruned) = prune(index);
    assert_eq!(pruned, 0);
    assert_eq!(counts(&index), before);
    assert!(counts(&index).iter().all(|(_, count)| *count > 0));
}

fn pages(index: &Index) -> i64 {
    index
        .connection
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap()
}

/// Records `pages` as the index's size after its last prune.
fn pruned_at(index: &Index, pages: i64) {
    index
        .connection
        .execute(
            "INSERT OR REPLACE INTO state (key, value) VALUES ('pruned_pages', ?1)",
            [pages.to_string()],
        )
        .unwrap();
}

/// The value of `key` in `state`.
fn state(index: &Index, key: &str) -> Option<String> {
    index
        .connection
        .query_row("SELECT value FROM state WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()
        .unwrap()
}

fn prune_if_doubled(index: Index) -> (Index, usize) {
    let writer = index.write().unwrap();
    let pruned = writer.prune_if_doubled().unwrap();
    (writer.decide(Vec::new()).unwrap().index, pruned)
}

#[test]
fn a_writer_prunes_once_the_index_has_doubled_since_the_last_prune() {
    let (index, unreachable) = filled_with(memory(), true);
    pruned_at(&index, pages(&index) / 2);
    let (index, pruned) = prune_if_doubled(index);
    assert_eq!(pruned, unreachable);
    assert_eq!(state(&index, "prunes").as_deref(), Some("1"));
    assert_eq!(
        state(&index, "pruned_pages"),
        Some(pages(&index).to_string())
    );
}

#[test]
fn a_new_index_records_its_size_at_its_first_writer_without_pruning() {
    let (index, _) = filled_with(memory(), true);
    let before = counts(&index);
    assert_eq!(state(&index, "pruned_pages"), None);
    let (index, pruned) = prune_if_doubled(index);
    assert_eq!(pruned, 0);
    assert_eq!(counts(&index), before);
    assert_eq!(state(&index, "prunes"), None, "no prune ran");
    assert_eq!(
        state(&index, "pruned_pages"),
        Some(pages(&index).to_string())
    );
}

#[test]
fn a_writer_does_not_prune_before_the_index_has_doubled() {
    let (index, _) = filled_with(memory(), true);
    let before = counts(&index);
    pruned_at(&index, pages(&index) / 2 + 1);
    let (index, pruned) = prune_if_doubled(index);
    assert_eq!(pruned, 0);
    assert_eq!(counts(&index), before);
}

#[test]
fn a_prune_gives_the_pages_it_frees_back_to_the_file() {
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = index.write().unwrap();
    let large = serde_json::json!({ "text": "x".repeat(400_000) });
    writer.put_body(blob(9), &canonical_json(&large)).unwrap();
    let index = writer.decide(Vec::new()).unwrap().index;
    let full = pages(&index);
    let (index, _) = prune(index);
    let free: i64 = index
        .connection
        .query_row("PRAGMA freelist_count", [], |row| row.get(0))
        .unwrap();
    assert_eq!(free, 0);
    assert!(
        pages(&index) < full / 2,
        "{} of {full} pages",
        pages(&index)
    );
}

/// Builds the views of one handle on an index file over a record nothing
/// reaches, lets a second handle on the file write, pruning when `prunes`,
/// then queries the views through the first.
fn queried_after_another_handle_writes(prunes: bool) -> Result<Rows, IndexError> {
    let cache = tempfile::tempdir().unwrap();
    let first = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let (first, _) = filled_with(first, true);
    let writer = first.write().unwrap();
    let first = writer
        .decide(vec![Resolved {
            source: Source::WorkingTree,
            path: "c.json".into(),
            logical: None,
            blob: Some(blob(3)),
            size: None,
            role: Some(Role::Json),
            schema: Some(blob(2)),
            kind: Kind::Record,
            code: None,
            change: Change::Uncommitted,
        }])
        .unwrap()
        .view()
        .unwrap();
    let second = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let writer = second.write().unwrap();
    if prunes {
        writer.prune().unwrap();
    } else {
        writer.put_blob(blob(6), Role::Json, &record()).unwrap();
    }
    writer.decide(Vec::new()).unwrap();
    first.query("SELECT type, status FROM entries", 10, None)
}

#[test]
fn a_query_after_another_process_prunes_is_refused_as_pruned() {
    let refused = queried_after_another_handle_writes(true).unwrap_err();
    assert!(matches!(refused, IndexError::Pruned), "{refused:?}");
}

#[test]
fn a_query_after_another_process_writes_without_pruning_reads_the_views() {
    let rows = queried_after_another_handle_writes(false).unwrap();
    assert_eq!(
        rows.rows,
        [[serde_json::json!("test/task/v1"), serde_json::json!("ok")]]
    );
}

#[test]
fn a_new_index_vacuums_incrementally() {
    let cache = tempfile::tempdir().unwrap();
    let index = Index::open(Some(cache.path()), Path::new(ROOT)).unwrap();
    let mode: i64 = index
        .connection
        .query_row("PRAGMA auto_vacuum", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, 2, "incremental");
}
