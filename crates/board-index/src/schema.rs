//! The tables. They are an interface agents write SQL against, so any change
//! to them is a new index format.

/// The index format, which names the index file. `open.rs` and
/// docs/architecture/index.md say what a new format does to older files.
pub const INDEX_VERSION: u32 = 29;

/// Every table, created in an empty database. Object ids are lowercase hex
/// and codes are their spelling, so a reader needs no Board types. A
/// record's references are numbered in the order the schema finds them, and
/// a Markdown file's links in document order. A kept decision's `detail`
/// is what check reports beyond the row, as JSON, or null, and its `change`
/// is null for a mounted entry, which is committed.
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
    size INTEGER,
    segment TEXT,
    code TEXT,
    PRIMARY KEY (tree, name)
) WITHOUT ROWID;
CREATE TABLE sources (
    source TEXT PRIMARY KEY,
    tree TEXT,
    repository TEXT NOT NULL
) WITHOUT ROWID;
CREATE TABLE mounts (
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    target TEXT,
    code TEXT,
    PRIMARY KEY (source, path)
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
-- What the last walk listed in each directory, so the next lists again only
-- the directories that changed: `path` is the directory's path in the
-- working tree, kept only when it is UTF-8, the stat columns its metadata before it was
-- listed, `listed_ns` when that walk began, and `items` each item's type
-- (d, l, f or ?) and name, each name ended by a zero byte.
CREATE TABLE directories (
    path TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    modified_ns INTEGER NOT NULL,
    changed_ns INTEGER NOT NULL,
    inode INTEGER NOT NULL,
    mode INTEGER NOT NULL,
    listed_ns INTEGER NOT NULL,
    items BLOB NOT NULL
) WITHOUT ROWID;
CREATE TABLE blobs (
    blob TEXT NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    type TEXT,
    code TEXT,
    reason TEXT,
    PRIMARY KEY (blob, role)
) WITHOUT ROWID;
-- A rowid table: SQLite writes rows of a few KiB faster to one than WITHOUT
-- ROWID, which suits small rows.
CREATE TABLE bodies (
    blob TEXT NOT NULL UNIQUE,
    body TEXT NOT NULL
);
-- What search reads in a Markdown file or a JSON file with no `$type`: its
-- first heading and its prose, cut at 64 KiB. A rowid table, as `bodies`.
CREATE TABLE texts (
    blob TEXT NOT NULL UNIQUE,
    heading TEXT,
    body TEXT NOT NULL
);
CREATE TABLE validations (
    record TEXT NOT NULL,
    schema TEXT NOT NULL,
    code TEXT,
    pointer TEXT,
    reason TEXT,
    failures INTEGER,
    PRIMARY KEY (record, schema)
) WITHOUT ROWID;
CREATE TABLE refs (
    record TEXT NOT NULL,
    schema TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    pointer TEXT NOT NULL,
    target TEXT NOT NULL,
    accepts TEXT,
    local INTEGER NOT NULL,
    PRIMARY KEY (record, schema, ordinal)
) WITHOUT ROWID;
CREATE TABLE links (
    blob TEXT NOT NULL,
    ordinal INTEGER NOT NULL,
    destination TEXT NOT NULL,
    PRIMARY KEY (blob, ordinal)
) WITHOUT ROWID;
-- A row with an empty path says the list under `scan` is stored. `repo`,
-- `follow` and `commit` are the mount the pointer holds, or null when it
-- holds none.
CREATE TABLE pointers (
    scan TEXT NOT NULL,
    path TEXT NOT NULL,
    logical TEXT,
    blob TEXT,
    repo TEXT,
    follow TEXT,
    commit_id TEXT,
    PRIMARY KEY (scan, path)
) WITHOUT ROWID;
-- What opening a mounted repository found, under a stamp of the files that
-- decide it: the repository's path, the pinned commit's root tree, and its
-- common git directory and stored `origin` when it has them.
CREATE TABLE probes (
    stamp TEXT PRIMARY KEY,
    path TEXT NOT NULL,
    root TEXT NOT NULL,
    common TEXT,
    origin TEXT
) WITHOUT ROWID;
-- The working tree's decision one build keeps for one board root, whose
-- rows `decided` holds under `slot`: `key` is the key of the tree they
-- describe, `frame` the digest of the moved prefixes, `head` the tree HEAD
-- named, `directories` each directory and submodule the walk saw and
-- `probes` each mount pointer's path and probe digest, each ended by a zero
-- byte.
CREATE TABLE slots (
    slot TEXT PRIMARY KEY,
    key TEXT NOT NULL,
    frame TEXT NOT NULL,
    head TEXT,
    directories BLOB NOT NULL,
    probes BLOB NOT NULL
) WITHOUT ROWID;
-- Each entry of a slot's tree: `entry` is the digest of the entry as
-- deciding reads it, and `reads` each name its decision read
-- (board_tree::reads), each ended by a zero byte.
CREATE TABLE inputs (
    slot TEXT NOT NULL,
    path TEXT NOT NULL,
    entry TEXT NOT NULL,
    reads BLOB NOT NULL,
    PRIMARY KEY (slot, path)
) WITHOUT ROWID;
CREATE TABLE decided (
    key TEXT NOT NULL,
    path TEXT NOT NULL,
    logical TEXT,
    blob TEXT,
    size INTEGER,
    role TEXT,
    schema TEXT,
    kind TEXT NOT NULL,
    code TEXT,
    detail TEXT,
    change TEXT,
    PRIMARY KEY (key, path)
) WITHOUT ROWID;
";

/// Each source a run reached: the key its decision is kept under when the
/// index holds it, and the key of a mounted source's pointer list; and the
/// stamp of each probe it used. Every transaction starts with them empty.
pub(crate) const REACHED: &str = "
CREATE TEMP TABLE IF NOT EXISTS reached (
    source TEXT PRIMARY KEY,
    key TEXT,
    scan TEXT
) WITHOUT ROWID;
DELETE FROM temp.reached;
CREATE TEMP TABLE IF NOT EXISTS probed (stamp TEXT PRIMARY KEY) WITHOUT ROWID;
DELETE FROM temp.probed;
";

/// Deletes the facts nothing current reaches: each tree beneath a source's
/// root, each object those trees hold, and each working-tree blob. A
/// validation and its references go unless both blobs are reached, a
/// decision, a slot or a pointer list unless this run reached its key, and
/// a probe unless this run used it.
pub(crate) const PRUNE: [&str; 12] = [
    "DELETE FROM main.trees WHERE tree NOT IN temp.kept",
    "DELETE FROM main.blobs WHERE blob NOT IN temp.kept",
    "DELETE FROM main.links WHERE blob NOT IN temp.kept",
    "DELETE FROM main.bodies WHERE blob NOT IN temp.kept",
    "DELETE FROM main.texts WHERE blob NOT IN temp.kept",
    "DELETE FROM main.validations
     WHERE record NOT IN temp.kept OR schema NOT IN temp.kept",
    "DELETE FROM main.refs WHERE record NOT IN temp.kept OR schema NOT IN temp.kept",
    "DELETE FROM main.decided
     WHERE key NOT IN (SELECT key FROM temp.reached WHERE key IS NOT NULL)",
    "DELETE FROM main.pointers
     WHERE scan NOT IN (SELECT scan FROM temp.reached WHERE scan IS NOT NULL)",
    "DELETE FROM main.probes WHERE stamp NOT IN temp.probed",
    "DELETE FROM main.slots
     WHERE slot NOT IN (SELECT key FROM temp.reached WHERE key IS NOT NULL)",
    "DELETE FROM main.inputs
     WHERE slot NOT IN (SELECT key FROM temp.reached WHERE key IS NOT NULL)",
];

/// Counts a prune in `state`, so a run can tell when another process has
/// pruned since it built its views.
pub(crate) const COUNTED: &str = "INSERT INTO main.state (key, value) VALUES ('prunes', 1)
     ON CONFLICT (key) DO UPDATE SET value = value + 1";

/// How many prunes the index has counted.
pub(crate) const PRUNES: &str =
    "SELECT CAST(coalesce(max(value), 0) AS INTEGER) FROM main.state WHERE key = 'prunes'";

/// Fills `temp.kept` with every object a source or the working tree
/// reaches, for [`PRUNE`].
pub(crate) const KEPT: &str = "
CREATE TEMP TABLE IF NOT EXISTS kept (object TEXT PRIMARY KEY) WITHOUT ROWID;
DELETE FROM temp.kept;
WITH RECURSIVE reached (tree) AS (
    SELECT tree FROM main.sources WHERE tree IS NOT NULL
    UNION
    SELECT t.object FROM main.trees AS t JOIN reached ON t.tree = reached.tree
    WHERE t.mode = 16384
)
INSERT INTO temp.kept
SELECT tree FROM reached
UNION SELECT object FROM main.trees WHERE tree IN (SELECT tree FROM reached)
UNION SELECT blob FROM main.worktree WHERE blob IS NOT NULL;
";

/// What one run decides about each entry, the view agents query, and each
/// fact table filtered to what the current entries reach. `resolved` is
/// every entry: each reached source's rows the index keeps, in source
/// order, then `fresh`, the rows decided this run and not kept. All live on
/// the connection that made them. An unqualified name finds a temporary
/// view before a table, so a query sees the same rows warm as cold. A body shows only while an entry holds its
/// blob as a record: the cache keeps it after the same bytes move to a path
/// where they are not one. `main.<table>` is the whole cache, which keeps
/// facts about content no longer in the tree so branches share them, until a
/// prune deletes what nothing current reaches.
pub(crate) const RESOLVED: &str = "
CREATE TEMP TABLE IF NOT EXISTS fresh (
    source TEXT NOT NULL,
    path TEXT NOT NULL,
    logical TEXT,
    blob TEXT,
    size INTEGER,
    role TEXT,
    schema TEXT,
    kind TEXT NOT NULL,
    code TEXT,
    change TEXT NOT NULL,
    PRIMARY KEY (source, path)
) WITHOUT ROWID;
DELETE FROM temp.fresh;
CREATE TEMP VIEW IF NOT EXISTS resolved AS
SELECT r.source, d.path, d.logical, d.blob, d.size, d.role, d.schema, d.kind, d.code,
    coalesce(d.change, 'committed') AS change
FROM temp.reached AS r CROSS JOIN main.decided AS d ON d.key = r.key
UNION ALL
SELECT * FROM temp.fresh;
CREATE TEMP VIEW IF NOT EXISTS decided AS
SELECT key, path, logical, blob, size, role, schema, kind, code
FROM main.decided WHERE key IN (SELECT key FROM temp.reached);
CREATE TEMP VIEW IF NOT EXISTS pointers AS
SELECT * FROM main.pointers WHERE scan IN (SELECT scan FROM temp.reached);
CREATE TEMP VIEW IF NOT EXISTS entries AS
SELECT
    r.source,
    r.path,
    r.logical,
    r.blob,
    r.size,
    r.kind,
    b.type,
    CASE WHEN r.code IS NULL THEN 'ok' ELSE 'degraded' END AS status,
    r.code,
    r.change,
    r.role,
    r.schema
FROM temp.resolved AS r
LEFT JOIN main.blobs AS b ON b.blob = r.blob AND b.role = r.role;
CREATE TEMP VIEW IF NOT EXISTS blobs AS
SELECT * FROM main.blobs
WHERE (blob, role) IN (SELECT blob, role FROM temp.resolved);
CREATE TEMP VIEW IF NOT EXISTS bodies AS
SELECT * FROM main.bodies
WHERE blob IN (SELECT blob FROM temp.resolved WHERE kind = 'record');
CREATE TEMP VIEW IF NOT EXISTS texts AS
SELECT * FROM main.texts
WHERE blob IN (SELECT blob FROM temp.resolved WHERE role IN ('markdown', 'json'));
CREATE TEMP VIEW IF NOT EXISTS records AS
SELECT e.source, e.path, e.logical, e.blob, e.type, e.status, e.code, b.body
FROM temp.entries AS e
LEFT JOIN main.bodies AS b ON b.blob = e.blob
WHERE e.kind = 'record';
CREATE TEMP VIEW IF NOT EXISTS validations AS
SELECT * FROM main.validations
WHERE (record, schema) IN (SELECT blob, schema FROM temp.resolved);
CREATE TEMP VIEW IF NOT EXISTS refs AS
SELECT * FROM main.refs
WHERE (record, schema) IN (SELECT blob, schema FROM temp.resolved);
CREATE TEMP VIEW IF NOT EXISTS links AS
SELECT * FROM main.links
WHERE blob IN (SELECT blob FROM temp.resolved WHERE role = 'markdown');
CREATE TEMP VIEW IF NOT EXISTS trees AS
WITH RECURSIVE reached (tree) AS (
    SELECT tree FROM main.sources WHERE tree IS NOT NULL
    UNION
    SELECT t.object FROM main.trees AS t JOIN reached ON t.tree = reached.tree
    WHERE t.mode = 16384
)
SELECT * FROM main.trees WHERE tree IN (SELECT tree FROM reached);
";
