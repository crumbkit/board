# jira

A Board plugin that records Jira issues as typed records. Other records can
reference an issue by its path, such as `/jira/acme/ACME-123`. `ref` writes
one issue's record. The plugin never creates, edits or comments on a Jira issue, and it
keeps no Jira state beyond the title.

## Enabling

Board has the plugin built in, so nothing is installed:

    board plugin enable jira

This writes the schema it ships to `.board/types/jira/`. An older install at
`${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/jira` makes `call` and
`plugin enable` fail with `board::plugin::duplicate`. Remove it; it is
usually a symlink to a checkout, so remove the link, not the checkout.

## Records

`jira/issue/v1` is one issue at `/jira/<site>/<key>`, such as
`/jira/acme/ACME-123`:

```json
{
  "$type": "jira/issue/v1",
  "site": "acme",
  "key": "ACME-123",
  "url": "https://acme.atlassian.net/browse/ACME-123",
  "title": "Fix the login redirect"
}
```

- `site` is the Jira Cloud site name, the lowercase part before
  `.atlassian.net`.
- `key` is a project key in capitals, a hyphen and an issue number.
- `url` is `https://<site>.atlassian.net/browse/<key>`.
- `title` is the issue's summary when the record was written. It is absent
  when the plugin could not read it.

## Capabilities

`board call jira.ref --key ACME-123 --site acme` writes the issue's record
with a put that expects no file there. Both flags are required. Before it
writes, it runs `acli jira workitem view <key> --fields summary --json` once, found on
the PATH a plugin's process sees, with 10 seconds to answer, and keeps the
work item's summary as the title. When `acli` is not on PATH, cannot start,
runs past 10 seconds, fails, prints no summary, or reads the work item from
another site, `ref` writes the record without a title and says why in
`title_note`.

When a record is already at the path, `ref` runs no `acli`, writes nothing and
reports that record with `written` false. To refresh a title, remove the
record and run `ref` again.

The output holds `path`, `site`, `key`, `url`, `title`, `written` and
`title_note`. `title` and `title_note` are null when there is none.

## Codes

- `jira::key::invalid`: the key is not a project key, a hyphen and a number,
  such as `ACME-123`. `fields.key` holds the value given.
- `jira::site::invalid`: the site is not a lowercase site name, such as
  `acme`. A full host such as `acme.atlassian.net` is refused.
- `jira::board::refused`: Board refused a read or the put. `fields.code`
  holds Board's code.
- `jira::record::unreadable`: the record already at the path lacks `site`,
  `key` or `url`. `fields.path` names it.
- `jira::call::unknown_capability`: the plugin offers only `ref`.

## Developing

The plugin is the crate `crates/board-plugin-jira`. It generates the
manifest's input and output schemas from Rust structs with schemars, and the
issue schema from `IssueRecord` in `src/records.rs`. Its `types/` directory
holds that schema as a committed golden, with the bytes the Python plugin
shipped, which a test compares with what the struct generates;
`SNAPSHOTS=overwrite` rewrites it. The tests of the plugin through the
binary are in `crates/board-cli/src/tests/built_in/jira.rs`, which puts a
stub `acli` on the PATH the plugin's process sees.
