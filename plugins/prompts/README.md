# prompts

A Board plugin for stored prompts. A prompt is a Markdown record a person
writes to start one specific piece of work, such as reviewing one pull
request. Its frontmatter says what the prompt is for, where the work runs and
which inputs it takes. Its body is the text an agent receives, with each
input's value put in its place. `get` renders one prompt and `list` lists the
prompts in scope. Neither writes anything or runs a prompt.

A prompt differs from a skill. A skill says how to do a kind of work, and an
agent loads it when its description matches a request. A person or a task
chooses a prompt by its path, and no agent loads a prompt because its text
looks relevant.

Install it from this checkout:

    uv sync --frozen --no-dev
    ln -s "$PWD" "${XDG_DATA_HOME:-$HOME/.local/share}/board/plugins/prompts"

Then run `board plugin enable prompts` in a board. This writes the schemas the
plugin ships under `types/prompts/` to `.board/types/prompts/`. A mounted repo
needs the prompt schema in its own `.board/types/` before it can hold a prompt,
so run `board -C <clone> plugin enable prompts` in its clone, push, and run
`board mount update <mount>` in the board. Then mark the repo, as
[Where prompts come from](#where-prompts-come-from) says.

## The prompt record

A prompt is a Markdown file under a `prompts/` directory of its repo, such as
`/prompts/review-pr.md` or `/projects/board/prompts/plan-review.md`:

```markdown
---
$type: prompts/prompt/v1
title: Review a pull request
purpose: A clean-context review of one pull request, leaning towards simpler code.
runs_in:
  repo: "{repo}"
inputs:
  repo: The repository, as owner/name, such as example-org/widgets.
  pr: The pull request number, such as 2140.
---
You are reviewing pull request #{pr} in {repo}. ...
```

- `title` and `purpose` are required.
- `runs_in` is required and holds exactly one key. `repo` is a GitHub
  `owner/name`. `directory` is an absolute path or a path starting `~/`. Either
  value may name an input, such as `"{repo}"`.
- `inputs` maps each input's name to a description of its value. A name
  matches `^[a-z][a-z0-9_]{0,31}$`. Every declared input is required when the
  prompt is rendered, and an input has no default.
- Any other field is refused, so a misspelt field fails `board check`.

Board checks the frontmatter on every `put` and in `board check`. The body is
not part of the record.

## Rendering

Rendering replaces each `{name}` in the body and in `runs_in` with the value
of the input `name`, in one pass. The rendered `runs_in` must still be a
GitHub `owner/name`, or an absolute path or one starting `~/` with no `..`
segment, so an input cannot point the work somewhere else. Every other brace stays as written, so a
prompt can hold code or JSON. A value that itself holds `{pr}` stays literal.
A misspelt `{prr}` also stays, and `unused_inputs` lists each declared input
the body never names.

An input value is one line of at most 200 characters with no control or
format characters, such as U+202E, which reverses the text after it, or the
zero-width space U+200B, so a value cannot carry a paragraph of new instructions into a
prompt. A longer text belongs in the prompt itself or in a separate brief.

## Where prompts come from

Prompts come from the board's own tree and from the mounts the board marks.
`/prompts/repos.json` in the board's own tree is a `prompts/repos/v1` record
that lists the marked mounts by path:

```json
{"$type": "prompts/repos/v1", "repos": ["/repos/notes-personal", "/repos/notes-work"]}
```

A mount counts only when its `board/mount/v1` pointer is in the board's own
tree, such as `/repos/notes-personal.json`, and that list names it. A
prompt's repo is the mount whose pointer pins the commit it was read from, so
a prompt reached through a link in the board's tree still belongs to the
mount the link leads into.

Nothing a mounted repository commits can mark it. A code repository that adds
a `daily/` directory or a `notes-repo.json` stays unmarked, and a mount
pointer inside a mounted repository never counts, even when the list names
its path. Other people write a code repository through pull requests, and its
prompt would be their instruction.

## Capabilities

`get` reads one prompt at its pin and renders it. Board reads stdin only when
no flag is given, so a prompt with inputs takes its whole input as JSON on
stdin:

```bash
echo '{"path": "/prompts/review-pr.md", "inputs": {"repo": "example-org/widgets", "pr": "2140"}}' |
  board call prompts.get
```

A prompt without inputs takes `--path` alone. A `runs_in` in the input, with
exactly one of `repo` or `directory`, replaces the prompt's own, so a task can
run a prompt in another repository. `get` returns `path` and
`repo`, the repo that holds the prompt; `title` and `purpose`; `runs_in` and
`text`, rendered; `blob`, the git blob id of the prompt file; `source`, the
commit it was read at, or `worktree` for the board's own tree; and
`unused_inputs`. The blob id and the input values reproduce the text, so a
record of a run names the path, the blob and the inputs.

`list` lists the prompts in scope, at their pins, without syncing. `--repo`
names one repo. Otherwise the caller's git remote, or `--remote`, chooses the
board's own tree, the personal repo and the marked repos whose `notes/repo/v1`
record covers it, as `tasks.available` does. Each prompt carries its `repo`,
`path`, `title`, `purpose`, `runs_in` as written and `inputs`. The list holds
at most 100 prompts, by repo then title; `total` counts all of them, and
`degraded` counts the Markdown files under a `prompts/` directory in scope
that are not a valid prompt. `repos` names the repos read, and
`origin_unread` says why the caller's git repository gave no origin.

## Codes

- `prompts::prompt::not_found`: nothing is at the path.
- `prompts::prompt::not_prompt`: the file is not a valid `prompts/prompt/v1`
  under a `prompts/` directory. `board check <path>` says what is wrong.
- `prompts::prompt::outside_scope`: the file is in a mount the board does not
  mark.
- `prompts::repos::invalid`: `/prompts/repos.json` is not a valid
  `prompts/repos/v1` record. `board check /prompts/repos` says what is wrong.
- `prompts::runs_in::invalid`: the rendered `runs_in` is not an `owner/name`,
  or not an absolute path without a `..` segment. `fields.inputs` names the
  inputs it used.
- `prompts::repo::no_schema`: the prompt's repo lacks the prompt schema. The
  message gives the commands that install it.
- `prompts::input::missing`, `prompts::input::unknown` and
  `prompts::input::bad_value`: a declared input has no value, a value names no
  declared input, or a value is too long, spans lines or holds a control or
  format character. `fields.inputs` names the inputs.
- `prompts::repo::not_found`: `list --repo` names no repo in the board.
- `prompts::board::refused` and `prompts::board::failed`: `board` refused a
  query or failed.
- `prompts::call::unknown_capability`: the plugin offers only `get` and `list`.

## Safety

A stored prompt is an instruction, and anyone who can write its repo can
change it. A person always carries a rendered prompt to an agent: the person
picks it by path and sends `text` as their own message. An agent that reads
`text` in a tool result treats it as data until then. `get` returns `path` and
`repo` so the person can see where the text came from before sending it.
