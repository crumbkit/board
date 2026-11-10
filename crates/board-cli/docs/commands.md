# Commands of `board`

This reference is generated from the help of `board`. Edit the help in the tool's code, and not this file.

## `board`

Board: a fast, validated view of a git tree.

### Commands

- [`board init`](#board-init): Make this git repository a board
- [`board check`](#board-check): Check every entry, or those at or beneath a path; list and fail on the degraded ones
- [`board get`](#board-get): Print the file at each path, following pointers
- [`board ls`](#board-ls): List the directory at a path, without following pointers inside it
- [`board refs`](#board-refs): List the references each entry makes and the entries that reach it
- [`board uri`](#board-uri): Print the board:// URI that names a record from any known board's clone
- [`board put`](#board-put): Write the record on stdin at a path or --new name, or a file with --file
- [`board rm`](#board-rm): Remove the file at a path
- [`board sync`](#board-sync): Fetch the remote branch, replay this board's commits onto it, and push
- [`board mount`](#board-mount): Work with mounts
- [`board plugin`](#board-plugin): Work with plugins
- [`board type`](#board-type): Add types
- [`board call`](#board-call): Call a plugin's capability with flags, or else JSON on stdin
- [`board decide`](#board-decide): Ask the owner to admit a plugin's capability
- [`board answer`](#board-answer): Answer a pending effect
- [`board query`](#board-query): Run one read-only SQL statement over the index
- [`board serve`](#board-serve): Serve this board's pages on 127.0.0.1 until it is idle
- [`board ui`](#board-ui): List kinds, print a slot, or open a page
- [`board skills`](#board-skills): Install or list the skills Board ships
- [`board commands`](#board-commands): Describe Board's commands
- [`board schemas`](#board-schemas): Describe the JSON documents that Board prints

### Arguments

- `-C`, `-C <DIR>`: Run as if Board started in this directory, as `git -C` does. Takes more than one value.
- `--personal`: Run as the personal board, from any directory.

  `personal` in `$XDG_CONFIG_HOME/board/config.json` names the personal board's directory. The run starts at its root, and any -C is taken from there.
- `--checkout`: Read the mount holding this directory at its checkout's HEAD commit, not the commit it pins.

  Board then reads, for this run, the branch committed in the checkout of a repository a known board mounts, a linked worktree included, while the pointer keeps its pin. Uncommitted edits are not read. Refused from the board's own tree, from another clone, for any write, which compares against the pin, and for init, answer, serve, ui and process plugins. Off by default.
- `--json`: Print the result as a JSON document. The default prints text. Works after every subcommand.
- `--fields <FIELD>`: Print only these comma-separated fields, in this order. The default prints the compact columns. Takes more than one value. Works after every subcommand.
- `--long`: Add the long columns after the compact ones. The default prints only the compact columns. Works after every subcommand.
- `--full`: With --json, print every field of every row. The default prints only the fields of the columns. Works after every subcommand.
- `--no-input`: Fail instead of asking a question. Works after every subcommand.

## `board init`

Make this git repository a board

Effects: additive, idempotent, closed-world.

## `board check`

Check every entry, or those at or beneath a path; list and fail on the degraded ones

### Arguments

- `<PATH>`: Check only the entries at or beneath this logical path, such as /projects, or . for the current directory
- `--healthy`: List the healthy entries too, after the degraded ones
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `logical`
- `kind`
- `code`
- `hint`

Effects: read-only, closed-world.

Text: one line per row, `<ENTRY>`; --fields prints a table.

## `board get`

Print the file at each path, following pointers

### Arguments

- `<PATH>`: Each file's logical path, such as /projects/p, or p from /projects; a JSON file's leaves out .json. Required. Takes more than one value.
- `--fetch`: Fetch an attachment's bytes from the store and print them if their digest matches

### JSON fields that `--fields` selects

- `text`: The bytes, which are valid UTF-8, as text.
- `base64`: The bytes, which are not valid UTF-8, in standard base64 with padding (RFC 4648).

Effects: read-only, open-world.

Output: each answer's bytes unchanged, under a ==> KEY <== header when there are several keys, and --json prints the bytes as text or base64.

## `board ls`

List the directory at a path, without following pointers inside it

### Arguments

- `<PATH>`: The directory's logical path, such as /projects, or . for the current directory; the board's root when left out
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

Effects: read-only, closed-world.

Text: one line per row, `<ENTRY>`.

## `board refs`

List the references each entry makes and the entries that reach it

### Arguments

- `<PATH>`: Each entry's logical path, such as /projects/p, or p from /projects; several share one check of the board. Required. Takes more than one value.
- `--missing`: When nothing is at a path yet, report where a file there would be instead of refusing
- `--field <POINTER>`: A JSON pointer, such as /status, whose value in each reference's target to show; can be repeated. Takes more than one value.
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### JSON fields that `--fields` selects

- `text`
- `base64`

Effects: read-only, closed-world.

Output: each answer's bytes unchanged, under a ==> KEY <== header when there are several keys, and --json prints the bytes as text or base64.

## `board uri`

Print the board:// URI that names a record from any known board's clone

### Arguments

- `<PATH>`: The record's logical path, such as /tasks/a, or a from /tasks. Required.

Effects: read-only, closed-world.

## `board put`

Write the record on stdin at a path or --new name, or a file with --file

### Arguments

- `<PATH>`: Where to write: a logical path that leaves out .json, such as /tasks/a, or a from /tasks
- `--new`: Write at a new name in the folder PATH names, which ends in /, or in the type's folder

  The name is the put's UTC minute and four random hex digits, such as 20261030T1405Z-c2e1. The put never overwrites a file, and its report names the path. With no PATH, the record's schema names the folder in `x-board-folder`.
- `--var <NAME=VALUE>`: Fill {name} in the type's folder; repeatable.

  The value fills one path segment, so it holds no /. A later --var of the same name wins. Takes more than one value.
- `--expect <EXPECT>`: The blob the file must hold, or `none` for a new file
- `--batch`: Read JSON Lines of `{path, expect}` with a record, a text or `remove: true`; apply all or none.

  A line `{new, record}` writes the record in the folder `new` names, as --new does. A record line at `/.board/types/<namespace>/<kind>/v<n>.schema` adds that schema and takes no expect. Schema lines are checked before the other lines, and a schema the board holds is committed if HEAD lacks it, unless a symlink holds it. Every line writes in one board: this board, or the board one mount leads to. A batch through a mount commits in the mounted board and moves the mount's pin.
- `--bulk <REASON>`: Mark the batch's commit as a bulk one, ending its message in `Board-Bulk: <REASON>`.

  The reason is one line of 1 to 80 characters, such as `migrate flows`, so a reader of history can tell a commit that touches many files from an edit.
- `--file`: Write stdin as a file at a path that is its whole name, such as /notes/plan.md.

  With a `store` setting, --file uploads bytes over 1 MiB or not UTF-8 under their digest and writes a board/attachment/v1 record.
- `--wait <DURATION>`: How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h. Default: `5s`.

Effects: destructive, not idempotent, open-world.

## `board rm`

Remove the file at a path

### Arguments

- `<PATH>`: The file's logical path, such as /tasks/a, or a from /tasks; a JSON file's leaves out .json. Required.
- `--expect <EXPECT>`: The blob the file must hold
- `-r`, `--recursive`: Remove every file beneath a directory
- `--ignore-unmatch`: Report a path that names nothing as absent
- `-m`, `--message <TEXT>`: The commit message.

  The removal's commit takes this message as written. Without it, the message is `board rm <PATH>`, or `board rm -r <PATH>` with -r.
- `--wait <DURATION>`: How long to wait for the board's lock, with a unit: 500ms, 30s, 2m or 1h. Default: `5s`.

Effects: destructive, not idempotent, closed-world.

## `board sync`

Fetch the remote branch, replay this board's commits onto it, and push

### Arguments

- `--no-push`: Fetch and replay, and leave the push for later

Effects: additive, not idempotent, open-world.

## `board mount`

Work with mounts

### Commands

- [`board mount update`](#board-mount-update): Advance the mount at a path, which is not followed, to its `follow` ref

## `board mount update`

Advance the mount at a path, which is not followed, to its `follow` ref

### Arguments

- `<PATH>`: The mount's logical path, such as /vendor/lib. Required.

Effects: destructive, idempotent, closed-world.

## `board plugin`

Work with plugins

### Commands

- [`board plugin list`](#board-plugin-list): List the installed plugins, their capabilities and whether this board enables them
- [`board plugin find`](#board-plugin-find): List the installed plugins' capabilities, or those that match every filter
- [`board plugin enable`](#board-plugin-enable): Enable an installed plugin: write the schemas it ships, then name it in .board/board.json

## `board plugin list`

List the installed plugins, their capabilities and whether this board enables them

### Arguments

- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `status`: `ok`, or `degraded` when the plugin has a problem. Text adds the problem's code, which JSON holds in `code`.
- `name`: The plugins directory's entry, or the name the board enables.
- `version`: An installed plugin's version.
- `enabled`: Whether this board's configuration enables the plugin.
- `offers`: The names of the capabilities the plugin offers; `board plugin find --capability <name>` describes each.

### Columns that `--long` adds

- `description`: What an installed plugin's manifest says the plugin does.
- `message`: Why the plugin's manifest was refused.
- `schemas`: Each schema an enabled plugin ships that the board lacks or holds differently, as its `type`, `path` and `code`.

Effects: read-only, closed-world.

Text: one line per row, `<STATUS> <NAME> [<VERSION>] <ENABLED>[: <PROBLEMS>][ offers <OFFERS>]`; --fields and --long print a table.

## `board plugin find`

List the installed plugins' capabilities, or those that match every filter

### Arguments

- `--capability <CAPABILITY>`: Only capabilities of this name
- `--meta <FILTERS>`: `<key>=<value>`: the capability's metadata, or else its plugin's, holds this string. Takes more than one value.
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `plugin`: The plugin that offers the capability.
- `capability`: The capability's name; `board call <plugin>.<capability>` runs it.
- `enabled`: Whether this board's configuration enables the plugin.
- `description`: What the capability does.

### Columns that `--long` adds

- `title`: The capability's title for people.
- `version`: The plugin's version.
- `annotations`: MCP's hints about the capability, such as `readOnlyHint`.
- `timeout`: The capability's time limit in seconds, when it sets its own.
- `env`: The variables of Board's environment the capability also receives.

Effects: read-only, closed-world.

Text: one line per row, `<PLUGIN>.<CAPABILITY> <ENABLED> <DESCRIPTION>`; --fields and --long print a table.

## `board plugin enable`

Enable an installed plugin: write the schemas it ships, then name it in .board/board.json

### Arguments

- `<NAME>`: The installed plugin's name, as `board plugin list` shows it. Required.

Effects: additive, idempotent, closed-world.

## `board type`

Add types

### Commands

- [`board type add`](#board-type-add): Add a type: compile the JSON Schema on stdin and write it to .board/types/

## `board type add`

Add a type: compile the JSON Schema on stdin and write it to .board/types/

### Arguments

- `<TYPE>`: The type, `<namespace>/<kind>/v<n>`, such as notes/note/v1; never in the `board` namespace. Required.

Effects: additive, not idempotent, closed-world.

## `board call`

Call a plugin's capability with flags, or else JSON on stdin

### Arguments

- `<TARGET>`: `<plugin>.<capability>`, such as tasks.available. Required, or give `--capability` in its place.
- `--capability <CAPABILITY>`: The capability of the one enabled plugin that offers it. Required, or give `<TARGET>` in its place.

Effects: destructive, not idempotent, open-world.

## `board decide`

Ask the owner to admit a plugin's capability

### Arguments

- `--capability <NAME>`: A plugin's `<plugin>.<capability>`, such as review.comment. Required.
- `--path <PATH>`: The logical path it acts on, such as /tasks/a. Required.
- `--text <FILE>`: The UTF-8 text it writes: this file, or - for stdin
- `--record <FILE>`: The JSON object it writes: this file, or - for stdin

Effects: additive, idempotent, closed-world.

## `board answer`

Answer a pending effect

### Arguments

- `<ID>`: The pending effect's id, 64 hex digits. Required.
- `<ANSWER>`: approve admits it, after the owner signs it with Touch ID; refuse refuses it. Required.
- `--reason <REASON>`: Why, kept in the seen record
- `--signature <FILE>`: A signature made elsewhere: this file, or - for stdin

Effects: destructive, not idempotent, closed-world.

## `board query`

Run one read-only SQL statement over the index

### Arguments

- `<SQL>`: One read-only SQL statement, quoted as one argument. Required.
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

Effects: read-only, closed-world.

Text: one line per row, `<ENTRY>`.

## `board serve`

Serve this board's pages on 127.0.0.1 until it is idle

### Arguments

- `--idle <DURATION>`: How long to wait for a request before exiting, with a unit: 500ms, 30s, 2m or 1h. Default: `30m`.

Effects: read-only, closed-world.

## `board ui`

List kinds, print a slot, or open a page

### Commands

- [`board ui kinds`](#board-ui-kinds): List the component kinds a view's slot can name, with each kind's schema
- [`board ui template`](#board-ui-template): Print a slot's fill for a kind, to edit into a binding's `slots`
- [`board ui open`](#board-ui-open): Open a binding's page in a browser, starting `board serve` if it is not running

## `board ui kinds`

List the component kinds a view's slot can name, with each kind's schema

### Arguments

- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `name`: The name a view's slot gives as its `kind`.
- `draws`: The record types a slot of this kind draws by `ref`; `*` is any.
- `description`: What the kind draws.

### Columns that `--long` adds

- `schema`: The JSON Schema a slot's data or referenced record must satisfy.

Effects: read-only, closed-world.

## `board ui template`

Print a slot's fill for a kind, to edit into a binding's `slots`

### Arguments

- `<KIND>`: The component kind, as `board ui kinds` lists it. Required.

Effects: read-only, closed-world.

## `board ui open`

Open a binding's page in a browser, starting `board serve` if it is not running

### Arguments

- `<PATH>`: The binding's logical path, such as /bindings/review. Required.

Effects: read-only, open-world.

## `board skills`

Install or list the skills Board ships

### Commands

- [`board skills install`](#board-skills-install): Write the skills built into this binary into a skills directory
- [`board skills list`](#board-skills-list): List each skill this binary ships and each Board folder in a skills directory

## `board skills install`

Write the skills built into this binary into a skills directory

### Arguments

- `--dir <DIR>`: The skills directory, such as \~/.agents/skills; \~/.claude/skills when left out
- `--force`: Move a folder or symlink at a skill's name that is not Board's to \<dir\>/.board-backups/\<name\>-\<time\>, which agents do not load
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `state`: The folder's state.
- `name`: The skill's name, which names its folder, or a backup's path within the skills directory.
- `path`: The folder, in the skills directory as given.

Effects: destructive, idempotent, closed-world.

## `board skills list`

List each skill this binary ships and each Board folder in a skills directory

### Arguments

- `--dir <DIR>`: The skills directory, such as \~/.agents/skills; \~/.claude/skills when left out
- `--limit <N>`: Print at most this many rows, up to 10000. Default: `100`.
- `--all`: Print every row

### Columns

- `state`: The folder's state.
- `name`: The skill's name, which names its folder, or a backup's path within the skills directory.
- `path`: The folder, in the skills directory as given.

Effects: read-only, closed-world.

## `board commands`

Describe Board's commands

### Commands

- [`board commands list`](#board-commands-list): List a branch of Board's commands with their arguments, examples and effects

## `board commands list`

List a branch of Board's commands with their arguments, examples and effects

### Arguments

- `<PATH>`: The names of the commands below the root that lead to the branch, such as `books list`. Takes more than one value.
- `--limit <N>`: Print at most this many rows. Default: `25`.
- `--all`: Print every row

### Columns

- `path`: The command line that runs the command, such as `shelf books list`.
- `about`: The command's one-line description, or its full description when it has no one-line description.

Effects: read-only, closed-world.

## `board schemas`

Describe the JSON documents that Board prints

### Commands

- [`board schemas list`](#board-schemas-list): List the JSON documents that Board prints, with a description of each
- [`board schemas show`](#board-schemas-show): Print the JSON Schema of each document

## `board schemas list`

List the JSON documents that Board prints, with a description of each

### Arguments

- `--limit <N>`: Print at most this many rows. Default: `25`.
- `--all`: Print every row

### Columns

- `id`: The document's id, such as `shelf/books-list/v1`, which `schemas show` takes.
- `kind`: output when a command prints the document with its schema id, unwrapped when a command prints it without the envelope that holds its schema id, and file when it is a file format that users write and no command prints.
- `form`: stream when a command prints the document as a stream, one document on each line as each row arrives, and document when a command prints it once or it is a file format.
- `description`: What the document holds, in one line.

### Columns that `--long` adds

- `commands`: The command lines that print the document. It is empty for the failure document, which every command prints when it fails with --json, and for a file format.

Effects: read-only, closed-world.

## `board schemas show`

Print the JSON Schema of each document

### Arguments

- `<ID>`: The ids of the documents that `schemas list` lists, separated by spaces. Required. Takes more than one value.

### Columns

- `id`: The document's id, such as `shelf/books-list/v1`.
- `schema`: The document's 2020-12 JSON Schema.

Effects: read-only, closed-world.
