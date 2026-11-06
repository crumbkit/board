# Skills

This file holds detail for docs/plugins.md, Skills. It says where a plugin's
skills live, how the build embeds them, and how `board skills install`
writes them.

A plugin's skill is `plugins/<plugin>/SKILL.md`, named after the plugin,
with its `README.md`, or a folder `plugins/<plugin>/skills/<skill>/` holding
a `SKILL.md`. The build script embeds every file of each skill whose
plugin's cargo feature the build enables, so a new skill needs no code.

`board skills install` writes each skill into `<dir>/<name>/`, with a
`.board-skill` file naming the version. `<dir>` is `~/.claude/skills` unless
`--dir` names another. A Board folder is replaced when it differs and
removed when its skill is retired. Another folder at a skill's name, in
any letter case, is refused with `board::skills::not_board`, whose
`folders` names it, unless `--force` replaces it. A symlink there is
removed, never followed. A failed read or write is
`board::skills::write_failed`. `board skills list` shows each skill's state.

Claude Code mods ship no skills.
