//! What to try next after each code (STANDARDS.md 25). The generic match
//! has no wildcard, so a new code does not compile until it has a hint.

use crate::{Code, Fields, Found, Known, LogicalPath};

impl Code {
    /// What to try next after an error with this code that names `fields`:
    /// with the values it names filled in where they are known, else the
    /// generic hint.
    pub fn hint(self, fields: &Fields) -> String {
        filled(self, fields).unwrap_or_else(|| generic(self).to_owned())
    }

    /// What to try next when the file at fault is in the board mounted at
    /// `mount`, which Board cannot write: where the hint names a file, it
    /// sends the fix to that board's repository.
    pub fn hint_in_mount(self, fields: &Fields, mount: &LogicalPath) -> String {
        mounted(self, fields, mount).unwrap_or_else(|| self.hint(fields))
    }
}

/// The hint for a mount at `path` that `board mount update` can mend.
fn updating(code: Code, path: &LogicalPath) -> String {
    let update = format!("`board mount update {path}`");
    match code {
        Code::MountCommitMissing => {
            format!("fetch the commit into the mounted repository, or run {update}")
        }
        Code::MountNotAncestor => {
            format!("run {update} to pin the mount to the mounted HEAD, then put again")
        }
        Code::MountStale => format!(
            "commit or discard the change in the mounted repository, run {update}, then read the record again and reapply your change"
        ),
        _ => format!(
            "write in the mounted repository and run {update}; a put writes through one mount whose board commits its puts and whose HEAD is on the branch the mount follows"
        ),
    }
}

/// The hint for `code` with the values in `fields` when the file at fault
/// is in the board mounted at `mount`, if it differs from the root board's.
fn mounted(code: Code, fields: &Fields, mount: &LogicalPath) -> Option<String> {
    let repository = format!("in the repository of the board mounted at {mount}");
    let schema = fields.schema.as_ref();
    Some(match code {
        Code::MountCommitMissing => format!(
            "the mount at {} is {repository}, which Board cannot write; fetch the commit \
             into the repository the mount names, or update the mount there",
            fields.path.as_ref()?
        ),
        Code::RecordUnknownType => format!(
            "add the type's schema at `{}` {repository}, or fix `$type` there",
            schema?
        ),
        Code::SchemaInvalid => format!(
            "fix the schema at `{}` {repository}; `board check {mount}/.board/types` says what failed",
            schema?
        ),
        Code::RefDangling => format!(
            "{} is {repository}, which Board cannot write; there, {}, then run `board mount update {mount}`",
            fields.path.as_ref()?,
            match fields.pointer.as_ref() {
                Some(pointer) => format!(
                    "create the record the reference at `{pointer}` names, or change the reference"
                ),
                None => "create the file the link names, or change the link's `path`".to_owned(),
            }
        ),
        Code::JsonParse
        | Code::JsonMergeConflict
        | Code::JsonDuplicateKey
        | Code::JsonNotUtf8
        | Code::JsonLossyNumber => format!(
            "{} is {repository}, which Board cannot write; fix the file there as the message says, then run `board mount update {mount}`",
            fields.path.as_ref()?
        ),
        _ => return None,
    })
}

/// The hint for `code` with the values in `fields`, when it names one and
/// `fields` holds it. A command it names in backticks must parse as it is.
fn filled(code: Code, fields: &Fields) -> Option<String> {
    let path = fields.path.as_ref();
    let pointer = fields
        .pointer
        .as_ref()
        .filter(|pointer| !pointer.is_empty());
    let capability = fields.capability.as_ref();
    Some(match code {
        Code::JsonMergeConflict => format!(
            "resolve the git merge conflict in the file, then run `board check {}`",
            path?
        ),
        Code::RecordUnknownType => format!(
            "add the type's schema at `{}`, or fix `$type`",
            fields.schema.as_ref()?
        ),
        Code::RepoUnreadable => format!(
            "check that `{}` can be read, then run the command again",
            fields.schema.as_ref()?
        ),
        Code::SchemaInvalid => format!(
            "fix the schema at `{}` so it compiles as JSON Schema 2020-12; `board check /.board/types` says what failed",
            fields.schema.as_ref()?
        ),
        Code::RecordInvalid => format!(
            "change the value at `{}` so the record satisfies its schema in `.board/types/`",
            pointer?
        ),
        Code::RecordNotARecord => format!(
            "only a record is put without `--file`, and nothing is put beneath a file; `board ls {}` shows what is there",
            path?
        ),
        Code::RefMalformed => format!(
            "change the reference at `{}` to a logical path that starts with `/`, such as `/projects/p`",
            pointer?
        ),
        Code::PathCaseCollision => format!(
            "move any files in `{}` beneath the spelling the message names, then write there",
            fields.directory.as_ref()?
        ),
        Code::PathMalformed => format!(
            "the logical path beneath `{}` would be over {} bytes; write to a path with fewer or shorter segments, or link to a shallower target",
            fields.directory.as_ref()?,
            crate::MAX_PATH_BYTES
        ),
        Code::PathNotFound => format!(
            "run `board ls {}` to see what is there; paths leave out `.json`",
            fields.ancestor.as_ref()?
        ),
        Code::RefDangling => dangling(fields)?,
        Code::MountCommitMissing
        | Code::PutThroughMount
        | Code::MountNotAncestor
        | Code::MountStale => updating(code, path?),
        Code::PutInvalid | Code::PutStale if !fields.refusals.is_empty() => BATCH.to_owned(),
        Code::PutStale => match (path?, fields.found?) {
            (path, Found::Blob(blob)) => format!(
                "read the record again with `board get {path}`, reapply your change, and put it with `board put {path} --expect {blob}`"
            ),
            (path, Found::Nothing) => format!(
                "nothing is at {path}; put it as a new file with `board put {path} --expect none`"
            ),
        },
        Code::AttachmentMissing | Code::AttachmentMismatch => reattach(path?),
        Code::PluginNotEnabled => format!(
            "add `{}` to `plugins` in `.board/board.json`",
            fields.plugin.as_ref()?
        ),
        Code::PluginSchemaDiffers => format!(
            "run `board plugin enable {}`, which replaces the board's copy with the schema the plugin ships",
            fields.plugin.as_ref()?
        ),
        Code::PluginAmbiguous => format!(
            "name the plugin as well, as in `board call {}.{}`; `board plugin find --capability {}` lists the plugins that offer it",
            fields.plugin.as_ref()?,
            capability?,
            capability?
        ),
        Code::PluginBadInput => format!(
            "give the input as the flags that `board call {}.{} -h` lists, or as JSON on stdin that matches the capability's `inputSchema`; `board plugin find --capability {} --json` shows the schema",
            fields.plugin.as_ref()?,
            capability?,
            capability?
        ),
        Code::QueryInvalid => match fields.known.as_ref()? {
            Known::Tables(tables) => format!(
                "the tables and views are {}; `board query --help` describes them",
                tables.join(", ")
            ),
            Known::Columns(tables) if tables.is_empty() => "run `board query \"SELECT name, sql FROM sqlite_master\"` for every table and its columns".to_owned(),
            Known::Columns(tables) => {
                let tables: Vec<String> = tables
                    .iter()
                    .map(|(table, columns)| format!("{table}: {}", columns.join(", ")))
                    .collect();
                format!("the columns are {}", tables.join("; "))
            }
        },
        _ => return None,
    })
}

/// What to do about an attachment at `path` whose bytes the store has lost
/// or altered.
fn reattach(path: &LogicalPath) -> String {
    format!(
        "put the file again with `board put {path} --file`, which uploads it under its digest and rewrites the record"
    )
}

/// What to do about a reference at `pointer` in the record at `path` that
/// leads nowhere, or, without a pointer, a link at `path` that does.
fn dangling(fields: &Fields) -> Option<String> {
    let path = fields.path.as_ref()?;
    let pointer = fields
        .pointer
        .as_ref()
        .filter(|pointer| !pointer.is_empty());
    Some(match pointer {
        Some(pointer) => format!(
            "create the record the reference at `{pointer}` names, or change the reference; `board refs {path}` lists the record's references"
        ),
        None => format!(
            "create the file the link at {path} names, or change the link's `path`; `board ls {}` shows the link",
            parent(path)
        ),
    })
}

/// What to do about a refused batch, whatever refused its lines.
const BATCH: &str = "fix each line listed, then send the whole batch again; nothing was written";

/// The directory that holds `path`: `/` for a path at the root.
fn parent(path: &LogicalPath) -> &str {
    match path.as_str().rsplit_once('/') {
        Some(("", _)) | None => "/",
        Some((parent, _)) => parent,
    }
}

/// The hint for `code` alone. A command it names in backticks, with
/// `<path>`, `<name>`, `<command>` or `<plugin>.<capability>` filled in,
/// must parse.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per code reads better than a table split across functions"
)]
fn generic(code: Code) -> &'static str {
    match code {
        Code::JsonParse => "fix the JSON syntax at the line and column the message gives",
        Code::JsonMergeConflict => {
            "resolve the git merge conflict in the file, then run `board check <path>`"
        }
        Code::JsonDuplicateKey => {
            "remove the repeated key; Board refuses JSON that names a key twice in one object"
        }
        Code::JsonTooLarge => {
            "Board reads at most 16 MiB of a file; split a record into records that reference each other"
        }
        Code::JsonNotUtf8 => "save the file as UTF-8",
        Code::JsonLossyNumber => {
            "write the number as a string; it does not fit a 64-bit integer or a double exactly"
        }
        Code::RecordBadType => {
            "set `$type` to `<namespace>/<kind>/v<n>` in lowercase, such as `lr/task/v1`"
        }
        Code::RecordUnknownType => {
            "add the type's schema at `.board/types/<namespace>/<kind>/v<n>.schema.json`, or fix `$type`"
        }
        Code::RecordInvalid => {
            "change the record to satisfy its schema in `.board/types/`; a refused `board put` names the field that fails"
        }
        Code::RecordNotARecord => {
            "only a record is put without `--file`, and nothing is put beneath a file; `board get <path>` prints any file's bytes, and `board ls <path>` shows what is there"
        }
        Code::ConfigNotFound => {
            "a board needs `.board/board.json` at its repository root, spelled exactly so; in a git repository, `board init` makes it a board; outside one, run `git init` first; for a board elsewhere, run `board -C <dir> check`"
        }
        Code::ConfigInvalid => {
            "`.board/board.json` holds `$type` `board/config/v1`, and may hold `commit_on_put` (true or false), `plugins` (distinct plugin names) and `store` (a `backend` and a `location`), nothing else"
        }
        Code::ConfigAmbiguous => {
            "choose a board with `board -C <dir> check`, naming one of the boards listed, or leave one mount of this repository among the boards in `$XDG_CONFIG_HOME/board/config.json`"
        }
        Code::SchemaInvalid => {
            "fix the schema so it compiles as JSON Schema 2020-12; the message says what failed"
        }
        Code::SchemaDiffers => {
            "a changed type is a new version: add this schema as the next one, such as v2 after v1, and move its records there; a type not yet released may instead be edited by hand"
        }
        Code::RefMalformed => {
            "a reference is a logical path that starts with `/`, such as `/projects/p`"
        }
        Code::RefWrongType => {
            "point the reference at a record of a type that the schema's `x-board-type` accepts"
        }
        Code::PathMalformed => {
            "a logical path starts with `/`, leaves out `.json`, and has no empty, `.` or `..` segment"
        }
        Code::PathNotFound => {
            "run `board ls <path>` on the parent directory to see what is there; paths leave out `.json`"
        }
        Code::PathShadowed => {
            "rename one of the entries that share this name, such as `x.json` beside a file `x`"
        }
        Code::PathCaseCollision => {
            "rename one of the names that differ only in case; a macOS working tree cannot hold both"
        }
        Code::RefDangling => {
            "create the record the reference names, or change the reference; `board refs <path>` lists them"
        }
        Code::MountInvalid => {
            "a link holds `$type` `board/mount/v1` and `path`; a mount holds `repo`, `follow` and `commit`"
        }
        Code::MountRepoMissing => {
            "clone the mounted repository to the path in `repo`, or change `repo`"
        }
        Code::MountCommitMissing => {
            "fetch the commit into the mounted repository, or run `board mount update <path>`"
        }
        Code::MountRemoteUnsupported => {
            "clone the repository on this machine and set `repo` to the clone's path"
        }
        Code::MountFollowMissing => {
            "create the ref that `follow` names in the mounted repository, or change `follow`"
        }
        Code::MountNotAncestor => {
            "run `board mount update <path>` to pin the mount to the mounted HEAD, then put again"
        }
        Code::MountStale => {
            "commit or discard the change in the mounted repository, run `board mount update <path>`, then read the record again and reapply your change"
        }
        Code::ResolveCycle => {
            "change one of the pointers so that they no longer lead back to each other"
        }
        Code::ResolveTooManyHops => {
            "point at the target more directly; resolution follows at most 40 pointers"
        }
        Code::RepoUnreadable => {
            "check that git can read the repository, for example with `git fsck`"
        }
        Code::RepoSubmodule => {
            "Board does not read submodules; mount the repository with a `board/mount/v1` record instead"
        }
        Code::IndexFailed => {
            "delete the board's directory under `$XDG_CACHE_HOME/board/` or `~/.cache/board/` and run the command again; the index is rebuilt from the tree"
        }
        Code::QueryInvalid => {
            "fix the statement as the message describes; Board runs one read-only SQL statement, and `board query --help` lists the tables"
        }
        Code::QueryTimeout => {
            "narrow the query with a WHERE clause or a join on a key; a query may run for 1 second"
        }
        // Only board-index raises it as a refusal, for a query's first row
        // or value; crumbkit gives get, refs and other output its own hint.
        Code::OutputOverBudget => {
            "one run prints up to 64 MiB of rows; select fewer or smaller columns, or add ORDER BY and page by key, as in WHERE (a, b) > (last a, last b)"
        }
        Code::PutStale => {
            "after `plugin enable`, run it again; after a put, read the record again with `board get <path>`, reapply your change, and put it with `--expect` set to the blob this error names"
        }
        Code::PutLocked => {
            "wait for the other write to finish, or give `put --wait` longer than 5s; or finish the merge, rebase, revert or cherry-pick in progress, then try again"
        }
        Code::PutThroughMount => {
            "write in the mounted repository and run `board mount update <path>`; a put writes through one mount whose board commits its puts and whose HEAD is on the branch the mount follows"
        }
        Code::PutSymlink => {
            "Board never writes through or over a symlink; replace the symlink the message names with a directory or file, or put to another path"
        }
        Code::PutInvalid => {
            "send one JSON object with a `$type` on stdin, such as `{\"$type\": \"lr/task/v1\", \"title\": \"a\"}`"
        }
        Code::PutNotAFile => {
            "put a file with `--file` at its whole name, such as `/notes/plan.md`; a name ending `.json` is read as a record, and a directory holds no bytes"
        }
        Code::PutNoIdentity => {
            "set `git config user.name` and `git config user.email`, or set `commit_on_put` to false in `.board/board.json`"
        }
        Code::PutHeadMoved => {
            "the files are written but not committed; check them with `git status` and commit them"
        }
        Code::PutFailed => {
            "check that the working tree is writable and the disk has space; the message says whether the commit was made"
        }
        Code::SyncNoRemote => {
            "clone the board from its remote, or set the branch's upstream with `git push -u <remote> <branch>`, then sync again"
        }
        Code::SyncFailed => {
            "git's stderr follows this error; `git remote -v` shows the remote, and `git fetch` run by hand shows what git asks for"
        }
        Code::SyncConflict => {
            "a commit here that is not pushed changed a file the remote also changed; resolve it in git, for example with `git pull --rebase`, then sync again"
        }
        Code::SyncEdited => {
            "the file was edited after its last commit and the remote changed it too; commit the edit with git or discard it with `git checkout -- <file>`, then sync again"
        }
        Code::SyncGaveUp => {
            "the remote refused every push; `board sync --no-push` brings this board up to date without pushing, and the commits stay for the next sync"
        }
        Code::RefsTargetChanged => {
            "the target's file changed during this run, so its values were not read; run the same board refs again. If every run refuses it, delete the board's directory under `$XDG_CACHE_HOME/board/` or `~/.cache/board/` so the index is rebuilt from the tree, and if it persists, report it"
        }
        Code::PluginNotInstalled => {
            "run `board plugin list` to see the installed plugins and their capabilities"
        }
        Code::PluginBadManifest => {
            "fix the plugin's `plugin.json` or the shipped schema the message names, or reinstall the plugin"
        }
        Code::PluginNotEnabled => "run `board plugin enable <name>`",
        Code::PluginSchemaMissing => {
            "run `board plugin enable <name>`, which writes the schemas the plugin ships"
        }
        Code::PluginSchemaDiffers => {
            "run `board plugin enable <name>`, which replaces the board's copy with the schema the plugin ships"
        }
        Code::PluginAmbiguous => {
            "name the plugin as well, as in `board call <plugin>.<capability>`; `board plugin find --capability <name>` lists the plugins that offer it"
        }
        Code::PluginBadInput => {
            "give the input as the flags that `board call <plugin>.<capability> -h` lists, or as JSON on stdin that matches the capability's `inputSchema`; `board plugin find --capability <name> --json` shows the schema"
        }
        Code::PluginBadOutput => {
            "the plugin answered outside its contract; report it to the plugin's author"
        }
        Code::PluginFailed => {
            "the plugin exited without an answer; the end of its stderr follows this error"
        }
        Code::PluginNoDatabase => {
            "Board could not make or read the plugin's database at the path the message names; check that its directory can be written, or delete the file, which is safe; then run the command again"
        }
        Code::PluginNotStarted => {
            "check that the program the plugin's `run` names exists and can be executed; a name without a `/` is looked up on `PATH`"
        }
        Code::PluginTimeout => {
            "the plugin ran for more than 30 seconds; narrow its input or report it to the plugin's author"
        }
        Code::PluginTooLarge => {
            "the plugin wrote more than 16 MiB; narrow its input or report it to the plugin's author"
        }
        Code::AttachmentMissing => {
            "nothing is at the location the attachment names; put the file again with `board put <path> --file`, which uploads it under its digest and rewrites the record, or restore the store"
        }
        Code::AttachmentMismatch => {
            "the bytes at the location the attachment names do not hash to its digest; put the file again with `board put <path> --file`, which uploads it under its digest and rewrites the record"
        }
        Code::PluginTooDeep => {
            "plugins called each other too deeply; look for a plugin that calls itself"
        }
        Code::CliUsage => {
            "run `board --help` for the commands, or `board <command> --help` for one command's arguments"
        }
    }
}
