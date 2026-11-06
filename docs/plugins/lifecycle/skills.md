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
or names none, as markers written before they named their skill do, is
listed as `copy`, reported by install as `kept`, and left alone. At a
shipped skill's name, a marker that names no skill counts as Board's.

Another folder at a skill's name, in any letter case, including one whose
marker names another skill, is refused with `board::skills::not_board`,
whose `folders` names it, unless `--force` replaces it. A symlink there is
removed, never followed. A failed read or write is
`board::skills::write_failed`. `board skills list` shows each skill's state.

Claude Code mods ship no skills.
