//! The tables. They are an interface agents write SQL against, so any change
//! to them is a new index format.

/// The index format. An index of any other format is deleted and rebuilt.
pub const INDEX_VERSION: &str = "1";

/// Every table, created in an empty database. Object ids are lowercase hex
/// and codes are their spelling, so a reader needs no Board types. A
/// record's references are numbered in the order the schema finds them.
pub(crate) const TABLES: &str = "
CREATE TABLE state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
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
