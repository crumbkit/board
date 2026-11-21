---
name: apply-standard
description: 'Audit a repository checkout against a written standard, a Markdown file whose tables list rules by ID, and record the result as a run of a suite in a board. Use it when the person asks to check, audit or review a repository against a standard, or to apply a standard to a repository. sample-standard.md in this skill is a standard to copy and edit.'
---

# Apply a standard

This skill checks a repository against the rules of a standard file. It
records each audit as a run of a suite with the runs plugin, so later audits
can be compared with it.

## What you need

- The person names a repository checkout on disk.
- Any git checkout works, including a worktree.
- The repository does not need to be mounted in the board.
- The person names a standard file.
- sample-standard.md, beside this file, is a sample standard to copy.
- The person names the board to record in.
- Pass the board to every command as `board -C <board>`.
- When the person names no board, use the board you are in.
- The board must enable the runs plugin.
- When `board put` refuses the suite with `board::record::unknown_type`, run
  `board -C <board> plugin enable runs`.
- Do not write the schema file that Board's hint names by hand.
  `board plugin enable runs` writes both of the plugin's schemas.

## The standard file

- A standard file is Markdown.
- Its rules are the rows of every table whose header has the columns `ID`,
  `Rule` and `Where it applies`.
- `ID` holds ASCII letters, digits, `-` and `_`.
- `Where it applies` holds one or more globs in inline code, relative to the
  repository's root, such as `**` for every file.
- Any other column, such as `How to check`, is guidance for you.
- Other tables and prose are not rules.

## Steps

1. Read the standard and list its rules in order. Stop before you write
   anything when the file has no table with the three columns, when two rows
   share an ID, or when a row's Where it applies cell holds no inline code.
   Tell the person which header or row to fix.
2. Run `git -C <checkout> rev-parse HEAD` to get the commit you audit. Run
   `git -C <checkout> status --porcelain` to find out whether the checkout
   has uncommitted changes. When the folder is not a git checkout, stop and
   ask the person for one. Change nothing in the repository.
3. Name the suite `<repository>-<standard>`.
   - `<repository>` is the last part of the URL that
     `git -C <checkout> remote get-url origin` prints, without `.git`.
     Every worktree of one repository has the same origin, so they all write
     to one suite.
   - When the checkout has no `origin` remote, `<repository>` is the name of
     the checkout's top folder, from `git -C <checkout> rev-parse --show-toplevel`.
   - `<standard>` is the standard file's name without `.md`.
   - Write both parts in lower case. Replace each run of characters other
     than letters, digits and `-` with one `-`, and drop a `-` at either end.
     For example, `Team Standard (v2).md` becomes `team-standard-v2`.
4. Run `board -C <board> get /suites/<repository>-<standard>`.
   - When the board holds no suite there, write one with
     `board -C <board> put /suites/<repository>-<standard>`, with the suite
     below on stdin and one case per rule ID.
   - When the suite's cases match the standard's rule IDs, leave the suite
     as it is.
   - When the standard has gained or lost rules, never change the suite
     silently. Tell the person which cases you would add and which you would
     remove.
   - Change the suite only when the person agrees. The change is one step:
     run `board -C <board> put /suites/<repository>-<standard>` with the
     suite on stdin and one case per rule ID of the standard as it is now.
   - Earlier runs keep their results, including the results for removed
     rules.
   - When the person does not agree, stop and write no run. A run names
     only rules that its suite lists.
5. For each rule, read the files its patterns match and decide whether the
   repository meets the rule.
   - List the checkout's files with
     `git -C <checkout> ls-files --cached --others --exclude-standard`, and
     match each pattern against that list.
   - The list holds tracked files and untracked files that git does not
     ignore. Ignored files, such as `target/` or `node_modules/`, never count.
   - A rule whose patterns match no file passes by absence. Record it as a
     result with outcome `pass` and the note
     "passes by absence: no file matches".
   - A rule you cannot judge from the files is skipped with a note that says
     why.
   - A rule the repository does not meet fails. Its note names the files and
     lines and says what the rule asks for.
   - Never copy a secret into a note. Name the file and line instead.
6. Write one run with
   `board -C <board> put --new --var suite=<repository>-<standard>`, with the
   run below on stdin. Leave out the rules that passed after you checked
   their files. Keep each rule that passed by absence.
7. Tell the person the run's path, which `board put` prints, the number of
   rules that failed, and each failure in one line.

When `board put` refuses a record, such as with `board::record::invalid` or
`board::put::no_identity`, tell the person Board's code and its hint. The
suite is written before the run, so a refused run leaves the suite in place.
Fix what Board names and write the run again.

## The suite

```json
{
  "$type": "runs/suite/v1",
  "subjects": ["<owner>/<name>"],
  "method": "Audited by the apply-standard skill against <standard file>. Each case is one rule.",
  "cases": [
    {"id": "SAMPLE-1"}, {"id": "SAMPLE-2"}, {"id": "SAMPLE-3"},
    {"id": "SAMPLE-4"}, {"id": "SAMPLE-5"}, {"id": "SAMPLE-6"}
  ],
  "measures": [
    {"name": "rules", "unit": "count"},
    {"name": "failures", "unit": "count"}
  ]
}
```

- `subjects` names the repository as `<owner>/<name>` when its `origin`
  remote gives one. Leave `subjects` out otherwise.
- `cases` lists every rule ID in the standard's order.

## The run

```json
{
  "$type": "runs/run/v1",
  "suite": "/suites/<repository>-<standard>",
  "by": {"program": "claude-code", "session": "<session>"},
  "started": "<started>",
  "finished": "<finished>",
  "scope": "full",
  "dirty": false,
  "versions": {"<owner>/<name>": "<commit>", "standard": "sha256:<hex>"},
  "measures": {"rules": 6, "failures": 1},
  "results": [
    {"case": "SAMPLE-2", "outcome": "fail", "note": "README.md lines 10 to 14 give two test commands, and neither runs the tests in web/."},
    {"case": "SAMPLE-5", "outcome": "pass", "note": "passes by absence: no file matches"}
  ],
  "summary": "One of six rules failed: no single command runs every test."
}
```

- `by.session` is the session's `local_` id. Leave `session` out when you do
  not know it.
- `started` and `finished` are UTC times, such as `2026-11-21T09:00:00Z`.
- `scope` is `full` when you judged every rule, and `targeted` when the
  person asked for some rules only.
- `dirty` is true when the checkout had uncommitted changes.
- `versions` names the commit you audited, keyed by the repository's
  `<owner>/<name>` or its name, and the standard file's SHA-256, from
  `shasum -a 256 <standard file>`.
- `measures` counts the rules in the standard and the rules that failed.
- `results` lists each failed rule with outcome `fail`.
- `results` lists each rule you did not judge with outcome `skipped`.
- `results` lists each rule that passed by absence with outcome `pass`.
- Each result has a `note`.
- `summary` is one sentence.

## Earlier results

- `board -C <board> runs latest --suite /suites/<repository>-<standard>`
  gives the latest full run of a clean commit and the rules it failed.
- `board -C <board> runs trend --suite /suites/<repository>-<standard> --measure failures`
  gives the failure count of each such run, newest first.
- A run that is `targeted` or `dirty` is not counted by either command.

## Rules for editors

The rules plugin can read the same standard file with
`board rules from-standards` and turn each row into a rule. Then
`board rules for-path` names the rule to an agent before it edits a file the
rule's patterns match. The rules plugin's README says how to lay out the
standards repository and the map that adopts the standard.
