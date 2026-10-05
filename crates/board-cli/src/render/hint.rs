//! What to try next after each code (STANDARDS.md 25). The match has no
//! wildcard, so a new code does not compile until it has a hint.

use board_model::Code;

/// The hint for `code`. A command it names in backticks, with `<path>`,
/// `<name>`, `<command>` or `<plugin>.<capability>` filled in, must parse.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per code reads better than a table split across functions"
)]
pub(crate) fn hint(code: Code) -> &'static str {
    match code {
        Code::JsonParse => "fix the JSON syntax at the line and column the message gives",
        Code::JsonMergeConflict => {
            "resolve the git merge conflict in the file, then run `board check <path>`"
        }
        Code::JsonDuplicateKey => {
            "remove the repeated key; Board refuses JSON that names a key twice in one object"
        }
        Code::JsonTooLarge => {
            "a record holds at most 16 MiB; split it into records that reference each other"
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
            "`board get` reads only records; run `board ls <path>` to see what is there"
        }
        Code::ConfigNotFound => {
            "run Board inside a git repository whose root holds `.board/board.json` with `{\"$type\": \"board/config/v1\"}`"
        }
        Code::ConfigInvalid => {
            "`.board/board.json` holds `$type` `board/config/v1`, and may hold `commit_on_put` (true or false) and `plugins` (distinct plugin names), nothing else"
        }
        Code::SchemaInvalid => {
            "fix the schema so it compiles as JSON Schema 2020-12; the message says what failed"
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
            "run one read-only SELECT statement; `board query --help` lists the tables"
        }
        Code::QueryTimeout => {
            "narrow the query with a WHERE clause or a join on a key; a query may run for 1 second"
        }
        Code::PutStale => {
            "read the record again with `board get <path>`, reapply your change, and put it with `--expect` set to the blob this error names"
        }
        Code::PutLocked => {
            "wait for the other write to finish, or finish the merge, rebase, revert or cherry-pick in progress, then try again"
        }
        Code::PutThroughMount => {
            "a mounted board is read-only here; write in the mounted repository, then run `board mount update <path>`"
        }
        Code::PutInvalid => {
            "send one JSON object with a `$type` on stdin, such as `{\"$type\": \"lr/task/v1\", \"title\": \"a\"}`"
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
        Code::PluginNotInstalled => {
            "run `board plugin list` to see the installed plugins and their capabilities"
        }
        Code::PluginBadManifest => {
            "fix the plugin manifest the message names, or reinstall the plugin"
        }
        Code::PluginNotEnabled => "add the plugin's name to `plugins` in `.board/board.json`",
        Code::PluginAmbiguous => {
            "name the plugin as well, as in `board call <plugin>.<capability>`; `board plugin find --capability <name>` lists the plugins that offer it"
        }
        Code::PluginBadInput => {
            "send JSON on stdin that matches the capability's `inputSchema`; `board plugin find --capability <name> --json` shows it"
        }
        Code::PluginBadOutput => {
            "the plugin answered outside its contract; report it to the plugin's author"
        }
        Code::PluginFailed => {
            "the plugin exited without an answer; the end of its stderr follows this error"
        }
        Code::PluginTimeout => {
            "the plugin ran for more than 30 seconds; narrow its input or report it to the plugin's author"
        }
        Code::PluginTooLarge => {
            "the plugin wrote more than 16 MiB; narrow its input or report it to the plugin's author"
        }
        Code::PluginTooDeep => {
            "plugins called each other too deeply; look for a plugin that calls itself"
        }
        Code::UsageInvalid => {
            "run `board --help` for the commands, or `board <command> --help` for one command's arguments"
        }
    }
}
