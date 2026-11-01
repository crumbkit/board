//! The degraded seed board: one board with an entry for every degraded code
//! `board check` reports, described once below. Each exhibit is written and
//! recorded with the code `check` must give it, so a test that compares the
//! two keeps the fixture from rotting. It is meant as the shared starting
//! point for tests of hints, transcripts and errors.

use board_model::{Code, ObjectId};

use crate::{Item, Scenario};

/// One entry of the seed board and what `check` says of it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Exhibit {
    /// Its logical path from the root board, or `None` when it has none.
    pub logical: Option<String>,
    /// Its path in the board that stores it: the root, or the mounted board.
    pub path: String,
    /// The code `check` gives it, or `None` when it is ok.
    pub code: Option<Code>,
}

/// Codes the CLI's recovery and twin checks cannot provoke through the
/// `board` command line, why, and where each is tested instead. The checks
/// provoke every other code, from the seed board or another small fixture.
pub const NOT_REPRODUCED: [(Code, &str); 19] = [
    (
        Code::AttachmentMissing,
        "the store must lose an object between the put and the fetch; \
         board-ops/src/attachment/tests.rs removes one",
    ),
    (
        Code::AttachmentMismatch,
        "the store must hold other bytes under the digest between the put and the fetch; \
         board-ops/src/attachment/tests.rs alters one",
    ),
    (
        Code::IndexFailed,
        "the index fails only when SQLite does, even in memory; \
         board-index/src/tests.rs maps SQLite's failures to it",
    ),
    (
        Code::PutNoIdentity,
        "whether git has an identity depends on the machine's configuration; \
         board-cli/tests/identity.rs runs board in a child process with an empty HOME",
    ),
    (
        Code::PutHeadMoved,
        "HEAD must move in the middle of a put; \
         board-tree/src/tests/commits.rs moves it between staging and commit",
    ),
    (
        Code::PutNameTaken,
        "every name a put draws must be taken, and the CLI draws from the clock and a random source; \
         board-ops/src/put/tests/new.rs draws fixed names",
    ),
    (
        Code::RefsTargetChanged,
        "a target must change between the check and the read of its fields; \
         board-cli/src/tests/refs.rs changes one between two answers",
    ),
    (
        Code::SyncTimeout,
        "the CLI's 60-second git limit is too slow for a test; \
         board-tree/src/tests/syncing.rs uses a 5-second limit",
    ),
    (
        Code::SyncKept,
        "a file must be edited during the sync's push; \
         board-ops/src/sync/tests.rs edits one in a pre-push hook",
    ),
    (
        Code::PluginTimeout,
        "the CLI's 30-second plugin limit is too slow for a test; \
         board-ops/src/call/tests.rs uses a 500 ms limit",
    ),
    (
        Code::UiOpenFailed,
        "ui open must fail to start a server or a browser, and a test must not start one; \
         board-serve/src/tests.rs pairs a binary that cannot start with a running server",
    ),
    (Code::ServeHost, SERVED),
    (Code::ServeOrigin, SERVED),
    (Code::ServeToken, SERVED),
    (Code::ServeCodeUnknown, SERVED),
    (Code::ServeCodeUsed, SERVED),
    (Code::ServeCodeExpired, SERVED),
    (Code::ServeUnknownRoute, SERVED),
    (
        Code::ServeFailed,
        "serve fails only when the system refuses a port, a file or random bytes; \
         board-serve/src/tests.rs runs the server",
    ),
];

const SERVED: &str = "the server gives it to an HTTP request, never to a command line; \
     board-serve/src/tests.rs pairs each refused request with an admitted twin";

const PUT: &str = "put refuses a write with it; check writes nothing";
const PLUGIN: &str = "a plugin command gives it; check runs no plugin";
const QUERY: &str = "query gives it; check runs no query";
const SETTINGS: &str =
    "put, decide or answer gives it for a plugin's effect; check asks for no effect";
const SYNC: &str = "sync gives it; check reads no remote";
const OPEN: &str = "ui open or the server gives it; check opens and serves nothing";

/// Codes `check` never gives an entry, and why. The seed board has an
/// exhibit for every other code, so a new code needs one or a line here.
pub const NOT_CHECKED: [(Code, &str); 69] = [
    (
        Code::AttachmentMissing,
        "only get --fetch gives it; check reads the attachment record and never the store",
    ),
    (
        Code::AttachmentMismatch,
        "only get --fetch gives it; check reads the attachment record and never the store",
    ),
    (
        Code::OutputOverBudget,
        "a run gives it once its output reaches the budget; check's listing is cut to fit instead",
    ),
    (
        Code::RefsTargetChanged,
        "only refs --field gives it, to a target whose file changed after the check",
    ),
    (
        Code::RecordNotARecord,
        "get gives it to a directory, and put to a path that is not a record file",
    ),
    (
        Code::ConfigNotFound,
        "a board without its configuration does not open, so it has no entries",
    ),
    (
        Code::ConfigAmbiguous,
        "a board that cannot be chosen does not open, so it has no entries",
    ),
    (
        Code::PathNotFound,
        "a command gives it to a path that names nothing; an entry names itself",
    ),
    (
        Code::MountFollowMissing,
        "mount update reads the followed ref; check reads the pinned commit",
    ),
    (
        Code::IndexFailed,
        "it fails the whole command, never one entry",
    ),
    (Code::QueryInvalid, QUERY),
    (Code::QueryTimeout, QUERY),
    (Code::SyncNoRemote, SYNC),
    (Code::SyncFailed, SYNC),
    (Code::SyncConflict, SYNC),
    (Code::SyncEdited, SYNC),
    (Code::SyncKept, SYNC),
    (Code::SyncGaveUp, SYNC),
    (Code::SyncTimeout, SYNC),
    (
        Code::SchemaDiffers,
        "type add refuses a schema with it; check compiles each schema alone",
    ),
    (Code::PutStale, PUT),
    (Code::PutLocked, PUT),
    (Code::PutThroughMount, PUT),
    (Code::MountNotAncestor, PUT),
    (Code::MountStale, PUT),
    (Code::PutSymlink, PUT),
    (Code::PutInvalid, PUT),
    (Code::PutNotAFile, PUT),
    (Code::PutNul, PUT),
    (Code::PutNoIdentity, PUT),
    (Code::PutNotAFolder, PUT),
    (Code::PutNameTaken, PUT),
    (Code::PutNoFolder, PUT),
    (Code::PutFolderUnfilled, PUT),
    (Code::PutVarUnused, PUT),
    (
        Code::RmDirectory,
        "rm refuses a directory with it; check removes nothing",
    ),
    (Code::SettingsAsked, SETTINGS),
    (Code::SettingsRefused, SETTINGS),
    (Code::SettingsFromPlugin, SETTINGS),
    (Code::SettingsNotFound, SETTINGS),
    (Code::SettingsNoPersonal, SETTINGS),
    (Code::SettingsUnsigned, SETTINGS),
    (Code::PutHeadMoved, PUT),
    (Code::PutFailed, PUT),
    (Code::PluginNotInstalled, PLUGIN),
    (Code::PluginNotEnabled, PLUGIN),
    (Code::PluginSchemaMissing, PLUGIN),
    (Code::PluginSchemaDiffers, PLUGIN),
    (Code::PluginAmbiguous, PLUGIN),
    (Code::PluginDuplicate, PLUGIN),
    (Code::PluginAliasConflict, PLUGIN),
    (Code::PluginBadInput, PLUGIN),
    (Code::PluginBadOutput, PLUGIN),
    (Code::PluginFailed, PLUGIN),
    (Code::PluginNotStarted, PLUGIN),
    (Code::PluginTimeout, PLUGIN),
    (Code::PluginTooLarge, PLUGIN),
    (Code::PluginTooDeep, PLUGIN),
    (
        Code::CliUsage,
        "the command line is refused before any board is read",
    ),
    (Code::UiNotABinding, OPEN),
    (Code::UiOpenFailed, OPEN),
    (Code::ServeHost, OPEN),
    (Code::ServeOrigin, OPEN),
    (Code::ServeToken, OPEN),
    (Code::ServeCodeUnknown, OPEN),
    (Code::ServeCodeUsed, OPEN),
    (Code::ServeCodeExpired, OPEN),
    (Code::ServeUnknownRoute, OPEN),
    (Code::ServeFailed, OPEN),
];

/// The seed board, and the board it mounts at `/mount/m`, which must outlive it.
#[derive(Debug)]
pub struct Seed {
    pub board: Scenario,
    pub mounted: Scenario,
    /// Every entry but those in `SUPPORT`, each with the code `check` gives it.
    pub exhibits: Vec<Exhibit>,
}

impl Default for Seed {
    fn default() -> Self {
        Self::new()
    }
}

/// What exhibits rely on: the configuration, two types and one project.
const SUPPORT: [(&str, &str); 4] = [
    (".board/board.json", r#"{"$type": "board/config/v1"}"#),
    (".board/types/lr/task/v1.schema.json", TASK_SCHEMA),
    (
        ".board/types/lr/project/v1.schema.json",
        r#"{"type": "object"}"#,
    ),
    ("projects/p.json", r#"{"$type": "lr/project/v1"}"#),
];

const TASK_SCHEMA: &str = r#"{
  "type": "object",
  "required": ["title"],
  "properties": {
    "title": {"type": "string"},
    "project": {"type": "string", "format": "board-ref", "x-board-type": "lr/project/v1"}
  }
}"#;

/// Exhibits in the root board that are text: path, text and code.
#[rustfmt::skip]
const TEXT: [(&str, &str, Option<Code>); 35] = [
    ("json/parse.json", "{", Some(Code::JsonParse)),
    ("json/conflict.json", "{\n<<<<<<< HEAD\n}\n", Some(Code::JsonMergeConflict)),
    ("json/duplicate.json", r#"{"a": 1, "a": 2}"#, Some(Code::JsonDuplicateKey)),
    ("json/lossy.json", r#"{"a": 18446744073709551616}"#, Some(Code::JsonLossyNumber)),
    ("frontmatter/parse.md", "---\n$type: lr/task/v1\na: 1\na: 2\n---\n", Some(Code::FrontmatterParse)),
    ("frontmatter/unsupported.md", "---\n$type: lr/task/v1\na: .inf\n---\n", Some(Code::FrontmatterUnsupported)),
    // Markdown whose frontmatter names a `$type` is a record.
    ("record/ok.md", "---\n$type: lr/task/v1\ntitle: t\nproject: /projects/p\n---\n# t\n", None),
    // A file with no `$type` is plain JSON, which `check` accepts.
    ("record/untyped.json", r#"{"title": "t"}"#, None),
    ("record/ok.json", r#"{"$type": "lr/task/v1", "title": "t", "project": "/projects/p"}"#, None),
    ("record/bad-type.json", r#"{"$type": "task"}"#, Some(Code::RecordBadType)),
    ("record/unknown.json", r#"{"$type": "lr/note/v1"}"#, Some(Code::RecordUnknownType)),
    ("record/invalid.json", r#"{"$type": "lr/task/v1", "title": 1}"#, Some(Code::RecordInvalid)),
    (".board/types/lr/broken/v1.schema.json", r#"{"type": 5}"#, Some(Code::SchemaInvalid)),
    ("record/broken.json", r#"{"$type": "lr/broken/v1"}"#, Some(Code::SchemaInvalid)),
    ("ref/malformed.json", r#"{"$type": "lr/task/v1", "title": "t", "project": "projects/p"}"#, Some(Code::RefMalformed)),
    ("ref/dangling.json", r#"{"$type": "lr/task/v1", "title": "t", "project": "/projects/q"}"#, Some(Code::RefDangling)),
    ("ref/wrong.json", r#"{"$type": "lr/task/v1", "title": "t", "project": "/ref/dangling"}"#, Some(Code::RefWrongType)),
    ("path/.json", "{}", Some(Code::PathMalformed)),
    ("path/x", "", Some(Code::PathShadowed)),
    ("path/x.json", "{}", Some(Code::PathShadowed)),
    ("link/broken.json", r#"{"$type": "board/mount/v1", "path": "/nowhere"}"#, Some(Code::RefDangling)),
    ("cycle/a.json", r#"{"$type": "board/mount/v1", "path": "/cycle/b"}"#, Some(Code::ResolveCycle)),
    ("cycle/b.json", r#"{"$type": "board/mount/v1", "path": "/cycle/a"}"#, Some(Code::ResolveCycle)),
    ("mount/invalid.json", r#"{"$type": "board/mount/v1", "path": "/x", "extra": 1}"#, Some(Code::MountInvalid)),
    ("mount/escape.json", r#"{"$type": "board/mount/v1", "path": "/../outside"}"#, Some(Code::MountInvalid)),
    (
        "mount/remote.json",
        r#"{"$type": "board/mount/v1", "repo": "https://example.com/b.git", "follow": "main", "commit": "0101010101010101010101010101010101010101"}"#,
        Some(Code::MountRemoteUnsupported),
    ),
    // Two repo records of one repository, and one of another.
    ("repos/a.json", r#"{"$type": "board/repo/v1", "remote": "example-org/a"}"#, Some(Code::RepoDuplicateRemote)),
    ("repos/again.json", r#"{"$type": "board/repo/v1", "remote": "example-org/a"}"#, Some(Code::RepoDuplicateRemote)),
    ("repos/b.json", r#"{"$type": "board/repo/v1", "remote": "example-org/b"}"#, None),
    // Only `.board/board.json` configures the board, but any file of its type must read as one.
    ("config/stray.json", r#"{"$type": "board/config/v1", "colour": "red"}"#, Some(Code::ConfigInvalid)),
    ("plugins/broken/plugin.json", r#"{"$type": "board/plugin-manifest/v1"}"#, Some(Code::PluginBadManifest)),
    ("ui/view.json", r#"{"$type": "ui/view/v1", "slots": [{"name": "s", "kind": "markdown"}]}"#, None),
    ("ui/unknown.json", r#"{"$type": "ui/view/v1", "slots": [{"name": "s", "kind": "chart"}]}"#, Some(Code::UiUnknownKind)),
    ("ui/mismatch.json", r#"{"$type": "ui/binding/v1", "view": "/ui/view", "slots": {}}"#, Some(Code::UiSlotMismatch)),
    ("ui/invalid.json", r#"{"$type": "ui/binding/v1", "view": "/ui/view", "slots": {"s": {"data": {"text": "x"}}}}"#, Some(Code::UiSlotInvalid)),
];

/// Exhibits in the root board that are symlinks: path, text and code. A
/// symlink to a file reads as that file.
#[rustfmt::skip]
const SYMLINKS: [(&str, &str, Option<Code>); 4] = [
    ("symlink/outside.json", "../../outside.json", Some(Code::SymlinkOutside)),
    ("symlink/loop.json", "loop.json", Some(Code::SymlinkLoop)),
    ("symlink/dangling.json", "nowhere.json", Some(Code::SymlinkDangling)),
    ("symlink/ok.json", "../record/untyped.json", None),
];

/// A mount of `repo` at `commit`, following `main`.
pub fn mount_of(repo: &str, commit: ObjectId) -> String {
    format!(
        r#"{{"$type": "board/mount/v1", "repo": "{repo}", "follow": "main", "commit": "{commit}"}}"#
    )
}

impl Seed {
    pub fn new() -> Self {
        let mounted = Scenario::new();
        // Names a case-insensitive file system cannot hold, and a blob the
        // commit names but does not store, can only be in a commit.
        let in_mount: [(&str, Item<'_>, Code); 3] = [
            ("A.json", Item::File("{}"), Code::PathCaseCollision),
            ("a.json", Item::File("{}"), Code::PathCaseCollision),
            ("gone.json", Item::MissingBlob, Code::RepoUnreadable),
        ];
        let items: Vec<(&[u8], Item<'_>)> = in_mount
            .iter()
            .map(|(path, item, _)| (path.as_bytes(), *item))
            .collect();
        let commit = mounted.commit_items(&items);
        let mut exhibits: Vec<Exhibit> = in_mount
            .into_iter()
            .map(|(path, _, code)| Exhibit {
                logical: Some(format!("/mount/m/{}", path.trim_end_matches(".json"))),
                path: path.to_owned(),
                code: Some(code),
            })
            .collect();

        let repo = mounted.path();
        let large = " ".repeat(16 * 1024 * 1024 + 1);
        let (no_repo, no_commit, mount) = (
            mount_of(&format!("{repo}/nowhere"), commit),
            mount_of(&repo, ObjectId::Sha1([9; 20])),
            mount_of(&repo, commit),
        );
        let mut root: Vec<(&str, Item<'_>, Option<Code>)> = TEXT
            .iter()
            .map(|(path, text, code)| (*path, Item::File(text), *code))
            .collect();
        root.extend([
            (
                "json/large.json",
                Item::File(&large),
                Some(Code::JsonTooLarge),
            ),
            (
                "json/latin1.json",
                Item::Bytes(b"\"\xe9\""),
                Some(Code::JsonNotUtf8),
            ),
            ("repo/nested", Item::Submodule, Some(Code::RepoSubmodule)),
            (
                "mount/repo.json",
                Item::File(&no_repo),
                Some(Code::MountRepoMissing),
            ),
            (
                "mount/commit.json",
                Item::File(&no_commit),
                Some(Code::MountCommitMissing),
            ),
            ("mount/m.json", Item::File(&mount), None),
        ]);
        root.extend(SYMLINKS.map(|(path, text, code)| (path, Item::Symlink(text), code)));
        // Resolving /hops/h0 follows 41 pointers, one more than the limit.
        let hops: Vec<(String, String)> = (0..=41)
            .map(|hop| {
                let text = if hop == 41 {
                    "{}".to_owned()
                } else {
                    format!(
                        r#"{{"$type": "board/mount/v1", "path": "/hops/h{}"}}"#,
                        hop + 1
                    )
                };
                (format!("hops/h{hop}.json"), text)
            })
            .collect();
        for (at, (path, text)) in hops.iter().enumerate() {
            let code = (at == 0).then_some(Code::ResolveTooManyHops);
            root.push((path, Item::File(text), code));
        }
        let board = Scenario::new();
        for (path, text) in SUPPORT {
            board.write(path, Item::File(text));
        }
        for (path, item, code) in root {
            board.write(path, item);
            let logical = match path.trim_end_matches(".json") {
                "path/" => None,
                name => Some(format!("/{name}")),
            };
            exhibits.push(Exhibit {
                logical,
                path: path.to_owned(),
                code,
            });
        }
        exhibits.extend(sealed(&board));
        Self {
            board,
            mounted,
            exhibits,
        }
    }
}

/// Writes a schema the walk's user cannot read and a record of its type, and
/// returns both as exhibits. Check reports both unreadable; a superuser reads
/// any file, so there both are ok.
fn sealed(board: &Scenario) -> [Exhibit; 2] {
    let (schema, record) = (
        ".board/types/lr/sealed/v1.schema.json",
        "record/sealed.json",
    );
    board
        .write(schema, Item::Unreadable(r#"{"type": "object"}"#))
        .write(record, Item::File(r#"{"$type": "lr/sealed/v1"}"#));
    let code = std::fs::File::open(board.workdir().join(schema))
        .is_err()
        .then_some(Code::RepoUnreadable);
    [schema, record].map(|path| Exhibit {
        logical: Some(format!("/{}", path.trim_end_matches(".json"))),
        path: path.to_owned(),
        code,
    })
}
