//! The tables. They are an interface agents write SQL against, so any change
//! to them is a new index format.

/// The index format. An index of any other format is deleted and rebuilt.
pub const INDEX_VERSION: &str = "3";

/// Every table, created in an empty database. Object ids are lowercase hex
/// and codes are their spelling, so a reader needs no Board types. A
/// record's references are numbered in the order the schema finds them.
pub(crate) const TABLES: &str = "
CREATE TABLE state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE trees (
    tree TEXT NOT NULL,
    name TEXT NOT NULL,
    mode INTEGER NOT NULL,
    object TEXT NOT NULL,
    segment TEXT,
    code TEXT,
    PRIMARY KEY (tree, name)
) WITHOUT ROWID;
CREATE TABLE worktree (
    path TEXT PRIMARY KEY,
    logical TEXT,
    code TEXT,
    blob TEXT,
    size INTEGER,
    modified_ns INTEGER,
    changed_ns INTEGER,
    inode INTEGER,
    mode INTEGER
) WITHOUT ROWID;
CREATE TABLE blobs (
    blob TEXT NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    type TEXT,
    code TEXT,
    PRIMARY KEY (blob, role)
) WITHOUT ROWID;
CREATE TABLE validations (
    record TEXT NOT NULL,
    schema TEXT NOT NULL,
    code TEXT,
    pointer TEXT,
    PRIMARY KEY (record, schema)
) WITHOUT ROWID;
CREATE TABLE refs (
    record TEXT NOT NULL,
    schema TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    pointer TEXT NOT NULL,
    target TEXT NOT NULL,
    accepts TEXT,
    PRIMARY KEY (record, schema, ordinal)
) WITHOUT ROWID;
";

/// What one run decides about each entry, and the view agents query. Both
/// live on the connection that made them, so nothing decided per run is
/// stored.
pub(crate) const RESOLVED: &str = "
CREATE TEMP TABLE IF NOT EXISTS resolved (
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    role TEXT,
    kind TEXT NOT NULL,
    code TEXT,
    change TEXT NOT NULL,
    PRIMARY KEY (source, path)
) WITHOUT ROWID;
DELETE FROM resolved;
CREATE TEMP VIEW IF NOT EXISTS entries AS
SELECT
    r.source,
    r.path,
    w.logical,
    w.blob,
    w.size,
    r.kind,
    b.type,
    CASE WHEN r.code IS NULL THEN 'ok' ELSE 'degraded' END AS status,
    r.code,
    r.change
FROM resolved AS r
LEFT JOIN main.worktree AS w ON r.source = 'worktree' AND w.path = r.path
LEFT JOIN main.blobs AS b ON b.blob = w.blob AND b.role = r.role;
";
