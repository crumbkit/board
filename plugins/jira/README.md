# jira

This plugin is still a Python program and is being ported to a Rust crate
built into Board (docs/plugins/python.md, docs/plugins/built-in/writing.md).

A Board plugin that records Jira issues as typed records. Other records can
reference an issue by its path, such as `/jira/acme/ACME-123`. `ref` writes
one issue's record. The plugin never creates, edits or comments on a Jira issue, and it
keeps no Jira state beyond the title.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/jira"

Then run `board plugin enable jira` in a board. This writes the schema the
plugin ships under `types/jira/` to `.board/types/jira/`.

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
writes, it runs `acli jira workitem view <key> --fields summary --json` once, with 10 seconds to answer, and keeps the work item's summary as the
title. When `acli` is not on PATH, fails, prints no summary, or reads the work
item from another site, `ref` writes the record without a title and says why
in `title_note`.

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
- `jira::board::refused` and `jira::board::failed`: `board` refused a read or
  the put, or failed.
- `jira::call::unknown_capability`: the plugin offers only `ref`.
