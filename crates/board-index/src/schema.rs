//! The tables. They are an interface agents write SQL against, so any change
//! to them is a new index format.

/// The index format. An index of any other format is deleted and rebuilt.
pub const INDEX_VERSION: &str = "7";

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
CREATE TABLE blobs (
    blob TEXT NOT NULL,
    role TEXT NOT NULL,
    kind TEXT NOT NULL,
    type TEXT,
    code TEXT,
    PRIMARY KEY (blob, role)
) WITHOUT ROWID;
CREATE TABLE bodies (
    blob TEXT PRIMARY KEY,
    body TEXT NOT NULL
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
CREATE TABLE extracted (
    plugin TEXT NOT NULL,
    version TEXT NOT NULL,
    blob TEXT NOT NULL,
    PRIMARY KEY (plugin, version, blob)
) WITHOUT ROWID;
";

/// What one run decides about each entry, the view agents query, and each
/// fact table filtered to what the current entries reach. All live on the
/// connection that made them, so nothing decided per run is stored. An
/// unqualified name finds a temporary view before a table, so a query sees
/// the same rows warm as cold; `main.<table>` is the whole cache, which
/// keeps facts about content no longer in the tree so branches share them.
pub(crate) const RESOLVED: &str = "
CREATE TEMP TABLE IF NOT EXISTS resolved (
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
DELETE FROM temp.resolved;
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
SELECT * FROM main.bodies WHERE blob IN (SELECT blob FROM temp.resolved);
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
CREATE TEMP VIEW IF NOT EXISTS extracted AS
SELECT * FROM main.extracted
WHERE blob IN (
    SELECT r.blob FROM temp.resolved AS r
    JOIN main.blobs AS b ON b.blob = r.blob AND b.role = r.role
    JOIN main.validations AS v ON v.record = r.blob AND v.schema = r.schema
    WHERE b.code IS NULL AND v.code IS NULL
);
CREATE TEMP VIEW IF NOT EXISTS trees AS
WITH RECURSIVE reached (tree) AS (
    SELECT tree FROM main.sources WHERE tree IS NOT NULL
    UNION
    SELECT t.object FROM main.trees AS t JOIN reached ON t.tree = reached.tree
    WHERE t.mode = 16384
)
SELECT * FROM main.trees WHERE tree IN (SELECT tree FROM reached);
";
