# Skills

This file holds detail for docs/plugins.md, Skills. It says where a plugin's
skills live, how the build embeds them, and how `board skills install`
writes them.

A plugin's skill is `plugins/<plugin>/SKILL.md`, named after the plugin,
with its `README.md`, or a folder `plugins/<plugin>/skills/<skill>/` holding
a `SKILL.md`. The build script embeds every file of each skill whose
plugin's cargo feature the build enables, so a new skill needs no code.

`board skills install` writes each skill into `<dir>/<name>/`, with a
`.board-skill` marker holding one line, `board <version> <skill>`, which
names the version that wrote it and the skill. `<dir>` is `~/.claude/skills`
unless `--dir` names another. A Board folder is replaced when it differs.
A folder for a skill Board no longer ships is removed only when its marker
names that skill under the folder's own name. A marked folder whose marker
names another skill, such as an edited copy of `changes` at `my-changes`,
or names none, is listed as `copy`, reported by install as `kept`, and
left alone.

Another folder at a skill's name, in any letter case, including one whose
marker names another skill or none, is refused with `board::skills::not_board`,
whose `folders` names it. With `--force`, install moves that folder to
`<dir>/.board-backups/<name>-<time>`, with the UTC time such as
`20261106T101500Z` and `-2`, `-3` and on added when that name is taken,
and reports it as `backed_up` with its path. Agents load only the folders
one level down in a skills directory, so they do not load a backup. A
symlink there is moved itself, never followed. List and install pass over
`.board-backups`. When the new copy cannot take the folder's place, install
moves the folder back, and if that fails too, the refusal names the backup.
A failed read or write is `board::skills::write_failed`. `board skills
list` shows each skill's state.

Claude Code mods ship no skills.
