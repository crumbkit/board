# Getting started

This page takes you from the Board dmg, Homebrew or a release folder to a
project, a task, a question and an eval in your own board.

## Choose how to install

- On an Apple silicon Mac, use the Board dmg. It installs `board`,
  `attention-sign` for Touch ID signing, Board's skills and its Claude Code
  mods.
- On an Intel Mac or on Linux, run `brew install crumbkit/tap/board`.
- The formula also works on an Apple silicon Mac when you want only the
  `board` command.
- The formula installs only `board`. Run `board skills install` after it to
  write Board's skills.
- Touch ID signing needs Board.app, so it needs the dmg.
- Install one of the two, never both. Each puts a `board` on your PATH, the
  dmg's in `~/.local/bin` and the formula's in Homebrew's `bin`, and the
  first on PATH runs, possibly an older version.
- Do not unpack a release archive by hand on a Mac. A browser quarantines
  the download, and macOS then blocks the unsigned `board` inside it.
  Homebrew does not quarantine it, and the dmg is notarised.

## Before you install

- The installer refuses unless `~/.local/bin` is on the PATH your login shell
  sets.
- Run `echo $PATH` to check it.
- If `~/.local/bin` is missing, run
  `echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.zprofile` and open a new
  terminal.
- Install Claude Code first, so that `claude` is on PATH.
- The skills Board installs are for Claude Code.
- The installer adds Board's Claude Code mods only when it finds `claude`.

## Install

- Get the dmg from the person who shared Board with you.
- Open the dmg, run Board.app and let it finish.
- Board.app runs Board's installer.
- The installer writes `board` and `attention-sign` in `~/.local/bin`.
- It keeps the release in `~/.local/share/board`.
- It writes every skill Board ships in `~/.claude/skills`.
- It installs Board's Claude Code mods when `claude` is on PATH.
- It ends with the warning "Enabled no plugins, because ... does not name a
  personal board".
- That warning is expected on a fresh home.
- The next section makes the board and enables its plugins.
- `board plugin list` lists the plugins Board ships, with none enabled,
  until you make a board.

### Install from a release folder

- A release shared as a folder, with no dmg, holds the same installer that
  Board.app runs.
- Run `<release folder>/install --hooks`, where `<release folder>` is the
  folder's path. It installs what Board.app installs, because Board.app
  always runs `install --hooks`.
- `--hooks` installs Board's Claude Code mods, which changes Claude Code's
  settings.
- `<release folder>/install` without `--hooks` writes `board`,
  `attention-sign` and the skills, and no mods.

## Make your board

Check that git knows your name and email. Each command prints a value:

```sh
git config --global user.name
git config --global user.email
```

- If one prints nothing, set it with
  `git config --global user.name 'Your Name'` or
  `git config --global user.email you@example.com`.
- Board commits every write.
- Board refuses a write with `board::put::no_identity` when git has no name
  or email.

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
- Each enable is a commit.
- Running the block again changes nothing.

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

- The second `tasks add` prints the task it wrote, with its path
  `/tasks/my-first-task.md`.
- `tasks tree` lists the project with the task under it.
- In Claude Code, the tasks skill does the same. Type: "Add a project called
  My first project to my board, with a task called My first task under it."

## Plan a project

- The project's plan goes in the body of its record.
- To write it, type in Claude Code: "Write this plan in the body of
  /tasks/my-first-project.md: ..."
- A decision is a `plans/decision/v1` under `/projects/<project>/decisions/`.
- A question waiting on a person is a `plans/question/v1` under
  `/projects/<project>/questions/`.
- `by.person` names who decided or asked, as a lowercase name or an email.

Record a decision, ask a question and answer it:

```sh
cd ~/board
me=$(git config --global user.email)
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo '{"$type": "plans/decision/v1", "decision": "The project keeps one list.", "why": "One list is easier to review.", "refs": ["/tasks/my-first-project.md"], "by": {"person": "'"$me"'"}, "decided": "'"$now"'"}' | board --personal put --new --var project=my-first-project
question=$(echo '{"project": "my-first-project", "question": {"question": "Who reviews the list each month?", "options": [{"id": "a", "text": "Me"}, {"id": "b", "text": "The team"}], "by": {"person": "'"$me"'"}}}' | board --personal --json call plans.ask | sed -n 's/.*"path":"\([^"]*\)".*/\1/p')
board --personal plans open --path /projects/my-first-project
board --personal plans answer --path "$question" --option a --words 'Me, on the first Monday.'
board --personal plans open --path /projects/my-first-project
```

- The block reads the question's path from the JSON that `plans.ask` prints.
- `plans open` lists the questions still waiting.
- `plans answer` records the option you chose and your words.
- After the answer, `plans open` lists no question.

## Test something

- An eval asks a question about a subject, such as `/topics/wc` for a tool
  with no repository.
- A `runs/suite/v1` at `/suites/<name>` says how it is tested.
- Each `runs/run/v1` under `/suites/<name>/runs/` records one run.

Record a test of whether `wc -l` counts a last line that has no newline:

```sh
cd ~/board
me=$(git config --global user.email)
now=$(date -u +%Y-%m-%dT%H:%M:%SZ)
echo '{"$type": "runs/suite/v1", "subjects": ["/topics/wc"], "method": "Count the lines of two lines whose last has no newline.", "cases": [{"id": "no-final-newline"}], "measures": [{"name": "lines", "unit": "lines"}]}' | board --personal put /suites/wc-last-line
echo '{"$type": "evals/eval/v1", "question": "Does wc -l count a last line that has no newline?", "subjects": ["/topics/wc"], "suites": ["/suites/wc-last-line"], "status": "planned"}' | board --personal put /evals/wc-last-line
lines=$(printf 'one\ntwo' | wc -l | tr -d ' ')
run=$(echo '{"$type": "runs/run/v1", "suite": "/suites/wc-last-line", "by": {"person": "'"$me"'"}, "started": "'"$now"'", "scope": "full", "dirty": false, "results": [{"case": "no-final-newline", "outcome": "pass", "measures": {"lines": '"$lines"'}}]}' | board --personal --json put --new --var suite=wc-last-line | sed -n 's/.*"display":"\([^"]*\)".*/\1/p')
[ -n "$run" ]
echo '{"$type": "evals/eval/v1", "question": "Does wc -l count a last line that has no newline?", "subjects": ["/topics/wc"], "suites": ["/suites/wc-last-line"], "runs": ["'"$run"'"], "status": "concluded", "verdict": "refuted", "conclusion": "wc -l counted '"$lines"' for two lines, because it counts newlines."}' | board --personal put /evals/wc-last-line
board --personal evals about --subject /topics/wc
```

- The suite goes first, so that the eval's `suites` names a record that
  exists and `board check` reports no `board::ref::dangling`.
- `put --new --var suite=wc-last-line` names the run by the time and a
  random suffix, and the block reads its path from the JSON it prints.
- `[ -n "$run" ]` stops the block when that `put` printed no path.
- The last `put` rewrites the eval as concluded, with its verdict and run.
- `evals about` lists the eval as concluded.
- In Claude Code, the evals skill does the same. Type: "Use the evals skill
  to record a test of whether `wc -l` counts a last line that has no
  newline."

## Push your board to a remote

- Make an empty repository on your git host.
- In `~/board`, run `git remote add origin <url>` with that repository's URL.
- Run `git push -u origin main` once.
- That push sets the upstream branch that `board sync` follows.
- Until `main` has an upstream, `board sync` refuses with
  `board::sync::no_remote`.
- After that, `board sync` in `~/board` pulls and pushes your board.

## What does not work yet

- Running Board.app again after `config.json` names your board enables every
  built-in plugin in it, not only the six.
- Each of those enables is a commit that adds the plugin's schemas.
- Nothing else needs doing about them.
- The tasks commands do not push your board.
- Once the board has a remote, as "Push your board to a remote" says, run
  `board sync` in `~/board` after you write.
- This page does not cover a shared board yet.
- `claude plugin list` lists Board's mods as `board-<name>@board`.
- Remove one with `claude plugin uninstall board-<name>@board`.
