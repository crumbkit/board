# board search

`board search '<query>'` prints the records of the board and its mounts that
match a query, one per line: the record's path, its short type and its title.
The query uses the search syntax Flare uses, so the same text finds records in
Board and flares in Flare.

```text
$ board search 'budget type:question'
/plans/questions/q1  question  Which budget line pays for the review?
/plans/questions/q2  question  Can the budget review wait a week?
```

## The syntax

A query is a list of parts separated by spaces. A record matches when every
part matches.

| Part | Matches |
| --- | --- |
| `budget` | A word. It matches a record whose path, title or body holds it, in any ASCII case. |
| `"budget review"` | A phrase in double quotes. It matches as one string, spaces included. |
| `type:question` | The kind of record, by a short name. Each short name matches every version of its type. |
| `from:erin` | Who wrote the record: the `by.person`, `by.agent` or `by.program` of its body, in any ASCII case. |
| `from:me` | The owner that `owner` names in `$XDG_CONFIG_HOME/board/config.json`. |
| `type:decision,answer` | A comma inside a filter means any of the values. |
| `-type:task` | A leading minus excludes what the part matches. It works before a word, a phrase or a filter. |

The short type names are:

| Name | Matches `$type` |
| --- | --- |
| `task` | `tasks/task/*` |
| `question` | `plans/question/*` |
| `answer` | `plans/answer/*` |
| `decision` | `plans/decision/*` |
| `change` | `changes/change/*` |
| `plan` | `changes/plan/*` |
| `eval` | `evals/eval/*` |
| `run` | `runs/run/*` |
| `artifact` | `artifacts/artifact/*` |
| `rule` | `rules/rule/*` |

A record's title is its `title` field. A record with no `title` uses the
first of `summary`, `decision`, `words` and `question` that it has, so a
question's title is its question and a decision's title is its decision.

Words match by substring, so `run` also matches `running` and `prune`. Word
boundaries come in a later version of the syntax. The body a word matches is
the record's JSON, so a word can also match a field's name, such as `status`.
A Markdown record, such as a task written as a `.md` file, has only its
frontmatter in that JSON, so a word matches its frontmatter fields and never
the prose under them. Search a Markdown record by the words of its title or
another frontmatter field.

Any other filter, such as `updated:7d` or `in:/tasks`, is refused. The rest
of Flare's syntax (dates, `in:`, `sort:` and the fields only Flare has) comes
in later versions.

## Order and pages

Results come in this order: first the records whose title holds the most of
the query's words and phrases, then those whose body holds the most, then by
path. An excluded word does not count.

`--limit` sets how many results one page holds: 50 when it is left out, and
at most 1000. When more records match, the text ends with a line that gives
the command for the next page, with `--limit` when the page used one other
than 50, and `--json` gives the cursor in `next`. Pass
the cursor to `--after` for the following page. A cursor holds the last
result's place in the order, so a record written between two pages can be
missed or shown twice.

## JSON

`board search --json '<query>'` prints `board/search-report/v1`. Its `data`
holds the same fields as Flare's `GET /api/search`:

- `query` is the canonical text: the parts joined by one space, field names
  in lower case, and values quoted only when they hold a space or a comma.
- `parsed` lists the parts, each with its `kind` (`word`, `phrase` or
  `filter`), its `text` or its `field` and `values`, whether it is `negated`,
  and its `span`. A span counts Unicode characters from 0, and its `end` is
  exclusive.
- `results` lists each record's `path`, `type` and `title`.
- `next` is the cursor for the next page, or `null` when this page is the
  last.

## Filters

`board search --filters` lists the filters and the short type names, so a
builder that writes queries for a person can offer them. With `--json` it
prints `board/search-report/v1`, whose `data` holds `fields`, each with its
`name`, `kind`, `values` and `about`, and `types`, each with its `name` and
`prefix`.

## Errors

A query that does not parse is refused before Board reads the board, with
`board::cli::usage` and exit code 2. The message names each error, and the
JSON failure's `fields` hold the `query` and an `errors` list. Each error has
a `code`, a `span` and a `message` that says how to fix it. The codes are the
ones in the case file that Board shares with Flare:

| Code | Cause |
| --- | --- |
| `unknown_field` | A filter the syntax does not have, such as `updated:`. |
| `unclosed_quote` | A quote with no closing quote. |
| `empty_value` | A filter with no value, or an empty value in a comma list. |
| `empty_phrase` | A phrase with nothing between its quotes. |
| `nothing_to_exclude` | A minus with nothing after it. |
| `too_long` | More than 1000 characters. |
| `unknown_type` | A `type:` value that is not a short type name. |
| `unknown_person` | `from:me` when the configuration names no owner. |

To use `from:me`, add the owner's name to the known boards file, such as
`{"owner": "olive"}` beside its other keys. A Board build older than the one
that added `board search` refuses a file that holds `owner`.

## How it runs

Board compiles the query to one SQL statement over the `records` view of the
index and runs it through the same snapshot as `board query`. Every value from
the query goes into the statement as a blob literal of its bytes in hex, so no
text from the query reaches the SQL as text. A search runs for at most one
second, as every query does, and is refused with `board::query::timeout` when
it runs longer. The refusal's hint says to add a filter, such as `type:task`
or `from:erin`, or to use a longer or less common word, because a word that
most records hold makes the search read and rank most of the board. The
statement checks the filters before the words, so a filter also saves reading
the bodies of the records it rules out.

## Examples

These ten examples run against a board with twelve records, and a test checks
that each returns the paths its comment names, in that order.

```sh
# Records that mention budget, best title matches first.
board search 'budget'
# /plans/answers/a1, /plans/questions/q1, /plans/questions/q2,
# /tasks/budget-review, /plans/decisions/d1, /changes/budget-report

# A phrase matches as one string.
board search '"the budget"'
# /plans/questions/q2, /plans/decisions/d1

# Questions about the budget, of any version of the question type.
board search 'budget type:question'
# /plans/questions/q1, /plans/questions/q2

# Questions Erin asked.
board search 'type:question from:erin'
# /plans/questions/q1, /plans/questions/q2

# Records the board's owner wrote.
board search 'from:me'
# /plans/answers/a1, /plans/decisions/d1, /plans/questions/q3

# Budget records that are not tasks.
board search 'budget -type:task'
# /plans/answers/a1, /plans/questions/q1, /plans/questions/q2,
# /plans/decisions/d1, /changes/budget-report

# Decisions and answers about the budget.
board search 'type:decision,answer budget'
# /plans/answers/a1, /plans/decisions/d1

# Hiring records that someone other than the owner wrote.
board search 'hiring -from:me'
# /tasks/hiring-plan, /plans/decisions/d2

# Records written by either of two programs.
board search 'from:claude-code,codex'
# /plans/decisions/d2, /suites/gates/runs/r1

# Changes that do not mention budget.
board search '-budget type:change'
# /changes/passkey-login
```
