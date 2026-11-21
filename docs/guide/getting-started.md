# Getting started

This page takes you from the Board dmg to a project, a task and a question in
your own board. A test runs every `sh` block under "Make your board", "Write a
first record" and "Plan a project", in order, in a fresh home.

## Before you install

- The installer refuses unless `~/.local/bin` is on the PATH your login shell
  sets.
- Run `echo $PATH` to check it.
- If `~/.local/bin` is missing, run
  `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile` and open a new
  terminal.
- Install Claude Code first, so that `claude` is on PATH. The skills are for
  Claude Code, and the installer adds Board's Claude Code mods only when it
  finds `claude`.

## Install

- Open the dmg, run Board.app and let it finish.
- The installer Board.app runs writes `board` and `attention-sign` in
  `~/.local/bin`.
- It keeps the release in `~/.local/share/board`.
- It writes every skill Board ships in `~/.claude/skills`.
- It installs Board's Claude Code mods when `claude` is on PATH.
- It ends with the warning "Enabled no plugins, because ... does not name a
  personal board". This is expected on a fresh home. The next section makes
  the board and enables its plugins.
- `board plugin list` fails with `board::config::not_found` until you make a
  board.

## Make your board

Check that git knows your name and email. Each command prints a value:

```sh
git config user.name
git config user.email
```

- If one prints nothing, set it with
  `git config --global user.name 'Your Name'` or
  `git config --global user.email you@example.com`.
- Board commits every write, and it refuses with `board::put::no_identity`
  when git has no name or email.

Make the board in `~/board` and enable the six plugins it needs:

```sh
mkdir -p ~/board
cd ~/board
git init -b main
board init
board plugin enable tasks
board plugin enable runs
board plugin enable evals
board plugin enable rules
board plugin enable plans
board plugin enable share
```

- `tasks` keeps projects and tasks.
- `plans` keeps decisions, questions and answers.
- `runs` and `evals` keep tests and their results.
- `rules` keeps requirements on files.
- `share` copies records into another board.
- Each enable is a commit. Running the block again changes nothing.

Name `~/board` as your personal board in `~/.config/board/config.json`:

```sh
config="$HOME/.config/board/config.json"
if [ -e "$config" ]; then
  echo "$config exists, so add \"personal\" and \"boards\" to it by hand"
else
  mkdir -p "$HOME/.config/board"
  printf '{"personal": "%s/board", "boards": ["%s/board"]}\n' "$HOME" "$HOME" > "$config"
fi
```

- When the file exists, the block leaves it unchanged.
- In that case, add `"personal": "/Users/<you>/board"` and add
  `"/Users/<you>/board"` to the `"boards"` list.
- `board --personal` then finds the board from any directory.

Check that the board opens:

```sh
board --personal check
```

## Write a first record

A project is a task with `kind: project`. Add a project and a task under it:

```sh
cd ~/board
project=$(board --personal --json tasks add --kind project --title 'My first project' | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
board --personal tasks add --parent "$project" --title 'My first task'
board --personal tasks tree --path "$project"
```

- `tasks add` prints the path it wrote, such as `/tasks/my-first-project.md`.
- `tasks tree` lists the project with the task under it.
- In Claude Code, the tasks skill does the same. Type: "Add a project called
  My first project to my board, with a task called My first task under it."

## Plan a project

- The project's plan goes in the body of its record. In Claude Code, type:
  "Write this plan in the body of /tasks/my-first-project.md: ..."
- A decision is a `plans/decision/v1` under `/projects/<project>/decisions/`.
- A question waiting on a person is a `plans/question/v1` under
  `/projects/<project>/questions/`.
- `by.person` names who decided or asked, as a lowercase name or an email.

Record a decision and ask a question:

```sh
cd ~/board
me=$(git config user.email)
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo '{"$type": "plans/decision/v1", "decision": "The project keeps one list.", "why": "One list is easier to review.", "refs": ["/tasks/my-first-project.md"], "by": {"person": "'"$me"'"}, "decided": "'"$now"'"}' | board --personal put --new --var project=my-first-project
echo '{"project": "my-first-project", "question": {"question": "Who reviews the list each month?", "options": [{"id": "a", "text": "Me"}, {"id": "b", "text": "The team"}], "by": {"person": "'"$me"'"}}}' | board --personal call plans.ask
board --personal plans open --path /projects/my-first-project
```

- `plans open` lists the questions still waiting.
- Answer one with
  `board --personal plans answer --path <question> --option a --words '<your words>'`.

## Test something

- The evals skill records a test as an eval, a suite and its runs.
- In Claude Code, type: "Use the evals skill to record a test of whether
  `wc -l` counts a last line that has no newline."

## What does not work yet

- Running Board.app again after `config.json` names your board enables every
  built-in plugin in it, not only the six. Each is a commit that adds the
  plugin's schemas. Nothing else needs doing.
- The tasks commands do not push your board. Once the board has a remote, run
  `board sync` in `~/board` after you write.
- `board sync` cannot make the first push to an empty remote. Run
  `git push -u origin main` once, then `board sync`.
- This page does not cover a shared board yet.
- `claude plugin list` lists Board's mods as `board-<name>@board`. Remove one
  with `claude plugin uninstall board-<name>@board`.
